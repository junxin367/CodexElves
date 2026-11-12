"""远程确认 Claude 的项目目录信任；不启动 CLI、不发送消息、不改工具审批权限。"""
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time


def config_path():
    override = os.environ.get("CLAUDE_CONFIG_DIR")
    if override:
        root = Path(override).expanduser()
        if not root.is_absolute():
            raise ValueError("Claude 配置目录必须为绝对路径")
        return root / ".claude.json"
    return Path.home() / ".claude.json"


def read_config(path, writable=False):
    if writable and path.is_symlink():
        raise RuntimeError("Claude 配置为符号链接，未自动修改目录信任")
    if not path.exists():
        return {}, None
    info = path.stat()
    if (not stat.S_ISREG(info.st_mode) or info.st_size > 16 * 1024 * 1024
            or writable and info.st_uid != os.getuid()):
        raise RuntimeError("Claude 配置无法安全读取，未修改目录信任")
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict) or not isinstance(value.get("projects", {}), dict):
        raise RuntimeError("Claude 配置格式无效，未修改目录信任")
    return value, info


def project_paths(cwd, managed=False):
    if not isinstance(cwd, str) or not cwd or not Path(cwd).is_absolute():
        raise ValueError("项目目录无效")
    directory = Path(cwd).resolve(strict=not managed)
    if directory.exists() and not directory.is_dir():
        raise ValueError("项目路径不是目录")
    boundary = directory
    if directory.is_dir():
        try:
            result = subprocess.run(["git", "-C", str(directory), "rev-parse", "--show-toplevel"],
                                    capture_output=True, timeout=5)
        except FileNotFoundError:
            result = None
        if result is not None and result.returncode == 0:
            boundary = Path(result.stdout.decode("utf-8").strip()).resolve(strict=True)
            if directory != boundary and boundary not in directory.parents:
                raise ValueError("项目的 Git 信任边界已变化")
    # 用户只确认具体项目；不能将一次远程确认扩大为主目录、文件系统或整个挂载盘的信任。
    if (boundary == Path("/") or boundary == Path.home().resolve()
            or len(boundary.parts) == 3 and boundary.parts[1] == "mnt"):
        raise ValueError("请选择具体项目目录，不能在此确认整个主目录或磁盘")
    return directory, boundary


def inspect(cwd, managed=False, file=None):
    directory, boundary = project_paths(cwd, managed)
    value, _ = read_config(file or config_path())
    current = directory
    trusted = False
    while True:
        entry = value.get("projects", {}).get(str(current), {})
        if isinstance(entry, dict) and entry.get("hasTrustDialogAccepted") is True:
            trusted = True
            break
        if current == boundary:
            break
        current = current.parent
    return {"ok": True, "trusted": trusted, "projectPath": str(directory), "trustPath": str(boundary)}


def confirm(cwd, expected, managed=False, file=None):
    path = file or config_path()
    directory, boundary = project_paths(cwd, managed)
    if expected != str(boundary):
        raise ValueError("项目目录已变化，请重新确认信任")
    if managed:
        directory.mkdir(parents=True, exist_ok=True)
    lock = Path(str(path) + ".lock")
    deadline = time.monotonic() + 5
    # 与原生 CLI 使用同一个 .claude.json.lock 目录锁；不删除或接管其他进程持有的锁。
    while True:
        try:
            lock.mkdir(mode=0o700)
            break
        except FileExistsError:
            if time.monotonic() >= deadline:
                raise RuntimeError("Claude 正在更新配置，请稍后重试目录信任")
            time.sleep(0.05)
    temporary = None
    try:
        value, info = read_config(path, writable=True)
        projects = value.setdefault("projects", {})
        entry = projects.setdefault(str(boundary), {})
        if not isinstance(entry, dict):
            raise RuntimeError("Claude 项目配置格式无效，未修改目录信任")
        if entry.get("hasTrustDialogAccepted") is not True:
            # 仅更新所确认目录的原生信任标记；账号、密钥、其他项目与工具权限均保留原值。
            entry["hasTrustDialogAccepted"] = True
            fd, temporary = tempfile.mkstemp(prefix=".claude-trust-", dir=path.parent)
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                os.fchmod(stream.fileno(), stat.S_IMODE(info.st_mode) if info else 0o600)
                json.dump(value, stream, ensure_ascii=False, indent=2)
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
            # 原生配置若在持锁期间仍被其他写入者替换，停止本次更新，不覆盖其新数据。
            current = path.stat() if path.exists() else None
            if ((info is None) != (current is None)
                    or info and current and (info.st_ino, info.st_mtime_ns, info.st_size)
                    != (current.st_ino, current.st_mtime_ns, current.st_size)):
                raise RuntimeError("Claude 配置已变化，请重新确认目录信任")
            os.replace(temporary, path)
            temporary = None
        return {"ok": True, "trusted": True, "projectPath": str(directory), "trustPath": str(boundary)}
    finally:
        if temporary is not None:
            Path(temporary).unlink(missing_ok=True)
        lock.rmdir()


def main():
    try:
        request = json.loads(sys.stdin.readline(32 * 1024))
        if not isinstance(request, dict) or type(request.get("managed", False)) is not bool:
            raise ValueError("目录信任参数无效")
        if request.get("confirm") is True:
            result = confirm(request.get("cwd"), request.get("expectedPath"), request.get("managed", False))
        else:
            result = inspect(request.get("cwd"), request.get("managed", False))
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
        # 原生配置包含认证信息，不转发异常对象、配置原文或 Git stderr。
        result = {"ok": False, "code": 409, "message": "无法安全确认或保存 Claude 目录信任，首条消息尚未发送，请核对后重试"}
    print(json.dumps(result, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
