// 远程页面统一使用主题设置的 Select；值更新、禁用选项和 change 事件在组件内处理。
// 弹层由宿主提供，共用现有的定位、搜索、键盘导航、焦点恢复和 dialog 顶层规则。
export function createSelectComponent({ element, icon, button, togglePopup, openPopup, refreshPopup = () => {} }) {
  function setSelectValue(node, value) {
    value = String(value ?? "");
    const selected = node.selectOptions.find(option => option.value === value)
      || node.selectOptions.find(option => option.value === "")
      || node.selectOptions.find(option => !option.disabled) || node.selectOptions[0];
    node.value = selected?.value ?? "";
    const label = selected?.label || "暂无可用选项";
    node.setAttribute("aria-description", `当前选择：${label}`);
    node.title = `${node.getAttribute("aria-label")}：${label}${selected?.description ? ` · ${selected.description}` : ""}`;
    const content = node.renderSelectValue?.(selected);
    node.replaceChildren(...(Array.isArray(content) ? content
      : [element("span", "select-label", label), icon("chevron")]));
    refreshPopup(node);
  }

  function setSelectOptions(node, options, value = node.value) {
    node.selectOptions = options.map(option => ({ ...option, value: String(option.value ?? "") }));
    setSelectValue(node, value);
  }

  function selectControl(id, label, options, value, className = "", renderValue = null) {
    const node = button("", label, () => togglePopup(node), `select-trigger ${className}`.trim());
    node.id = id;
    node.setAttribute("aria-haspopup", "listbox");
    node.setAttribute("aria-expanded", "false");
    node.renderSelectValue = renderValue;
    setSelectOptions(node, options, value);
    node.onkeydown = event => {
      if (node.disabled || event.isComposing) return;
      if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault();
        openPopup(node, { edge: event.key === "Home" ? "first" : event.key === "End" ? "last" : null });
      }
    };
    return node;
  }

  function commitSelectValue(node, value) {
    // 用户选择只触发一次 change；程序同步、重复选择和禁用项不能触发设置保存或切换目标。
    value = String(value ?? "");
    // 提示词优化沿用注入版的手填模型能力；仅显式提供自定义选项校验的 Select 可以接收目录外的值。
    if (!node.disabled && !node.selectOptions.some(option => option.value === value)) {
      const custom = node.selectCustomOption?.(value);
      if (custom?.value === value && !custom.disabled) node.selectOptions.push(custom);
    }
    if (node.disabled || node.value === value
        || !node.selectOptions.some(option => option.value === value && !option.disabled)) return false;
    setSelectValue(node, value);
    node.dispatchEvent(new Event("change", { bubbles: true }));
    return true;
  }

  return { selectControl, setSelectValue, setSelectOptions, commitSelectValue };
}
