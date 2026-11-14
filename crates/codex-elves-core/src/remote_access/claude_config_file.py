"""在所选 WSL 用户内原子更新 Claude 设置，不返回配置内容或凭据。"""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile

MAX_BYTES = 2 * 1024 * 1024
MODES = {"", "default", "manual", "acceptEdits", "plan", "bypassPermissions", "dontAsk", "auto"}


class ConfigError(Exception):
    def __init__(self, code, message):
        self.code = code
        self.message = message


def read_file(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return "", None
    if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_BYTES or info.st_uid != os.getuid():
        raise ConfigError(403, "Claude 配置必须为当前 WSL 用户拥有的不超过 2 MiB 的普通文件")
    raw = path.read_bytes()
    if len(raw) > MAX_BYTES:
        raise ConfigError(413, "Claude 配置超过 2 MiB")
    try:
        return raw.decode("utf-8"), info
    except UnicodeDecodeError:
        raise ConfigError(422, "Claude 配置需要使用 UTF-8 编码") from None


def revision(text, info):
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    return hashlib.sha256(f"{str(info is not None).lower()}:{digest}".encode("utf-8")).hexdigest()


def update(path, changes, expected):
    if not path.is_absolute() or path.name != "settings.json":
        raise ConfigError(400, "Claude 配置路径无效")
    if not isinstance(changes, dict) or len(changes) != 1:
        raise ConfigError(400, "只允许修改一个已支持的 Claude 设置")
    if set(changes) == {"awaySummaryEnabled"}:
        if type(changes["awaySummaryEnabled"]) is not bool:
            raise ConfigError(400, "回顾开关必须为布尔值")
    elif set(changes) == {"permissionMode"}:
        if not isinstance(changes["permissionMode"], str) or changes["permissionMode"] not in MODES:
            raise ConfigError(400, "Claude 权限模式无效")
    else:
        raise ConfigError(400, "不支持的 Claude 设置")
    text, info = read_file(path)
    if revision(text, info) != expected:
        raise ConfigError(409, "实例配置已变化，请重新读取后再保存")
    if info and not stat.S_IMODE(info.st_mode) & 0o222:
        raise ConfigError(403, "实例配置文件为只读，未修改原文件")
    try:
        document = json.loads(text.lstrip("\ufeff")) if text.strip() else {}
    except ValueError:
        raise ConfigError(422, "Claude 配置格式无效，未覆盖原文件") from None
    if not isinstance(document, dict):
        raise ConfigError(422, "Claude 配置必须为 JSON 对象")
    if "awaySummaryEnabled" in changes:
        if "awaySummaryEnabled" in document and type(document["awaySummaryEnabled"]) is not bool:
            raise ConfigError(422, "Claude 离开期间回顾配置必须为布尔值")
        document["awaySummaryEnabled"] = changes["awaySummaryEnabled"]
    else:
        permissions = document.setdefault("permissions", {})
        if not isinstance(permissions, dict):
            raise ConfigError(422, "Claude permissions 配置格式无效")
        if changes["permissionMode"]:
            permissions["defaultMode"] = changes["permissionMode"]
        else:
            permissions.pop("defaultMode", None)
    ending = "\r\n" if "\r\n" in text else "\n"
    output = json.dumps(document, ensure_ascii=False, indent=2).replace("\n", ending) + ending
    if text.startswith("\ufeff"):
        output = "\ufeff" + output
    raw = output.encode("utf-8")
    if len(raw) > MAX_BYTES:
        raise ConfigError(413, "Claude 配置超过 2 MiB")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    temporary = None
    try:
        fd, temporary = tempfile.mkstemp(prefix=".codex-elves-settings-", dir=path.parent)
        with os.fdopen(fd, "wb") as stream:
            # 用户要求原生配置保持原权限和其他设置；Windows UNC 权限只表示只读，
            # 不能复制 Linux 0600 等模式。必须在所属 WSL 用户内设置权限并原子替换。
            if info and os.fstat(stream.fileno()).st_gid != info.st_gid:
                os.fchown(stream.fileno(), -1, info.st_gid)
            os.fchmod(stream.fileno(), stat.S_IMODE(info.st_mode) if info else 0o600)
            if info:
                for name in os.listxattr(path, follow_symlinks=False):
                    os.setxattr(stream.fileno(), name, os.getxattr(path, name, follow_symlinks=False))
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        current, current_info = read_file(path)
        metadata = lambda value: None if value is None else (
            value.st_ino, value.st_mtime_ns, value.st_mode, value.st_uid, value.st_gid)
        if revision(current, current_info) != expected or metadata(current_info) != metadata(info):
            raise ConfigError(409, "实例配置已变化，请重新读取后再保存")
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            Path(temporary).unlink(missing_ok=True)
    return {"ok": True}


def main():
    try:
        request = json.loads(sys.stdin.readline(16 * 1024))
        result = update(Path(request["path"]), request["changes"], request["expectedRevision"])
    except ConfigError as failure:
        result = {"ok": False, "code": failure.code, "message": failure.message}
    except PermissionError:
        result = {"ok": False, "code": 403, "message": "无法保存 Claude 配置，请检查所选 WSL 用户的目录权限"}
    except (OSError, ValueError, KeyError, TypeError):
        # 配置与异常可能含凭据；只返回固定错误说明，不打印路径、原文或底层异常。
        result = {"ok": False, "code": 503, "message": "无法安全保存 Claude 配置，请重新读取核对"}
    print(json.dumps(result, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
