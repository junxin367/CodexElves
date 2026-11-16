// 通用弹层宿主：Select 与按钮菜单共用的定位、搜索、键盘导航、焦点恢复与 dialog 顶层规则。
// 业务浮层（如输入区模型/Skill 菜单）通过 registerAppMenuCloser 注册，打开弹层或确认框前统一收起。

import { menuHorizontalLeft } from "./position.js";
import { button, element } from "./dom.js";
import { icon } from "./icons.js";

export let activePopup = null;
export function setActivePopup(value) { return activePopup = value; }
// 打开通用弹层或确认框前先收起业务浮层（输入区模型/Skill 菜单）；具体实现由入口启动时注册，
// 组件本身不依赖业务模块。注册前为空操作，入口须在任何弹层打开前完成注册。
export let closeAppMenus = () => {};
export function registerAppMenuCloser(close) { closeAppMenus = close; }
// Select 选项的提交由 ui/select.js 在加载时注册；弹层只负责展示、搜索与键盘导航，不反向依赖 Select 实现。
let commitSelection = (_trigger, _value) => false;
export function registerSelectCommitter(commit) { commitSelection = commit; }
export function closePopup(restoreFocus = false) {
  if (!activePopup) return;
  if (activePopup.submenuTimer) clearTimeout(activePopup.submenuTimer);
  if (activePopup.submenu) closePopupSubmenu();
  const { node, trigger, dismiss } = activePopup;
  activePopup = null;
  trigger.setAttribute("aria-expanded", "false");
  trigger.removeAttribute("aria-controls");
  if (node.hasAttribute("popover") && node.matches(":popover-open")) node.hidePopover();
  dismiss?.remove();
  node.remove();
  if (restoreFocus && trigger.isConnected && !trigger.disabled) trigger.focus();
}
export function togglePopup(trigger) {
  if (activePopup?.trigger === trigger) closePopup(true);
  else openPopup(trigger);
}
export function openPopup(trigger, { query = "", edge = null, focus = true, position = null } = {}) {
  if (trigger.disabled || !trigger.isConnected) return;
  closePopup(); closeAppMenus();
  const node = element("div", `ui-popup${trigger.popupClassName ? ` ${trigger.popupClassName}` : ""}`);
  node.id = "active-popup";
  const selecting = !!trigger.selectOptions;
  node.setAttribute("aria-label", trigger.getAttribute("aria-label"));
  if (trigger.popupTitle) node.append(element("div", "popup-title", trigger.popupTitle));
  let help = null;
  if (trigger.popupHelp) {
    help = element("a", "popup-help", trigger.popupHelp.label);
    help.href = trigger.popupHelp.href; help.target = "_blank"; help.rel = "noopener noreferrer";
    help.append(icon("window"));
    // 用户要求 Codex 权限菜单的“了解更多”紧随标题，不在列表底部重复出现。
    if (trigger.popupHelpAfterTitle) node.append(help);
  }
  const list = element("div", "popup-list");
  list.setAttribute("role", selecting ? "listbox" : "menu");
  list.setAttribute("aria-label", trigger.getAttribute("aria-label"));
  let input = null;
  if (trigger.selectSearchable) {
    input = element("input", "popup-search");
    input.type = "search"; input.placeholder = trigger.selectSearchPlaceholder || "搜索项目名称或路径"; input.autocomplete = "off";
    input.setAttribute("aria-label", trigger.selectSearchLabel || "搜索可选项目"); input.value = query;
    input.oninput = () => { if (activePopup) { activePopup.query = input.value; activePopup.index = 0; renderPopup(); } };
    node.append(input);
  }
  const note = trigger.popupNote ? element("div", "popup-note", trigger.popupNote) : null;
  // 用户要求 Claude 权限菜单的说明位于选项上方；其他弹层继续沿用底部说明。
  if (note && trigger.popupNotePosition === "top") node.append(note);
  node.append(list);
  if (note && trigger.popupNotePosition !== "top") node.append(note);
  if (help && !trigger.popupHelpAfterTitle) node.append(help);
  const choices = selecting ? trigger.selectOptions : trigger.menuOptions;
  activePopup = { node, trigger, list, input, query, position, index: Math.max(0, choices.findIndex(option => option.value === trigger.value)), choices: [] };
  trigger.setAttribute("aria-expanded", "true"); trigger.setAttribute("aria-controls", node.id);
  const host = trigger.closest("dialog") || document.body;
  if (trigger.popupDismissLayer) {
    const dismiss = element("div", "session-tools-menu-dismiss");
    dismiss.onpointerdown = event => { event.preventDefault(); event.stopPropagation(); };
    dismiss.onclick = event => {
      event.preventDefault(); event.stopPropagation(); closePopup(true);
    };
    activePopup.dismiss = dismiss;
    host.append(dismiss);
  }
  if (typeof node.showPopover === "function") node.setAttribute("popover", "manual");
  host.append(node);
  renderPopup();
  if (activePopup?.node !== node) return;
  if (node.hasAttribute("popover")) node.showPopover();
  const buttons = [...list.querySelectorAll("button")];
  const enabled = buttons.filter(option => !option.disabled);
  const selected = buttons[activePopup.index];
  const target = edge === "last" ? enabled.at(-1) : edge === "first" ? enabled[0]
    : selected && !selected.disabled ? selected : enabled[0];
  activePopup.index = Math.max(0, buttons.indexOf(target));
  if (focus) target?.focus();
  fitPopup();
  node.onkeydown = event => {
    if (event.isComposing || !activePopup) return;
    if (activePopup.submenu?.node.contains(event.target)) return;
    if (activePopup.submenu && ["Escape", "ArrowLeft"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation(); closePopupSubmenu(true); return;
    }
    const options = [...list.querySelectorAll("button:not(:disabled)")];
    if (event.key === "ArrowRight" && document.activeElement?.getAttribute("aria-haspopup") === "menu") {
      event.preventDefault(); event.stopPropagation();
      document.activeElement.click();
    } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) && !(event.target === input && ["Home", "End"].includes(event.key))) {
      event.preventDefault();
      if (!options.length) return;
      const index = options.indexOf(document.activeElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
        : index < 0 ? event.key === "ArrowDown" ? 0 : options.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : options.length - 1)) % options.length;
      activePopup.index = next;
      options[next]?.focus(); options[next]?.scrollIntoView({ block: "nearest" });
    } else if (event.target === input && event.key === "Enter") {
      event.preventDefault();
      const selected = list.querySelectorAll("button")[activePopup.index];
      (selected && !selected.disabled ? selected : options[0])?.click();
    } else if (event.key === "Tab") {
      if (help && event.target !== help && !event.shiftKey) {
        event.preventDefault(); help.focus();
      } else if (help && event.target === help && event.shiftKey) {
        event.preventDefault(); options.at(-1)?.focus();
      } else closePopup(true);
    } else if (event.target !== input && event.key.length === 1 && !event.ctrlKey && !event.metaKey && event.key !== " ") {
      const start = options.indexOf(document.activeElement) + 1;
      const ordered = [...options.slice(start), ...options.slice(0, start)];
      ordered.find(option => option.textContent.trim().toLowerCase().startsWith(event.key.toLowerCase()))?.focus();
    }
  };
}
export function renderPopup() {
  if (!activePopup) return;
  const { trigger, list, query, submenu } = activePopup;
  const restoreFocus = list.contains(document.activeElement);
  const focused = restoreFocus ? document.activeElement.dataset.value : null;
  const choices = (trigger.selectOptions || trigger.menuOptions).filter(option =>
    option.footer || !query || `${option.label} ${option.description || ""} ${option.searchText || ""}`.toLowerCase().includes(query.trim().toLowerCase()));
  const custom = trigger.selectCustomOption?.(query);
  if (custom && !trigger.selectOptions.some(option => option.value === custom.value)) choices.push(custom);
  activePopup.choices = choices;
  list.replaceChildren();
  const hasFooter = choices.some(option => option.footer);
  list.classList.toggle("has-footer", hasFooter);
  const options = hasFooter ? element("div", "popup-options") : list;
  if (hasFooter) list.append(options);
  if (!choices.some(option => !option.footer)) options.append(element("p", "popover-empty muted",
    query ? trigger.selectSearchLabel ? "没有匹配的选项，请修改搜索条件" : "没有匹配的项目，请修改搜索条件"
      : hasFooter ? "本机暂无可用项目" : "暂无可用选项"));
  choices.forEach(option => {
    if (option.separatorBefore) {
      const separator = element("div", "popup-separator");
      separator.setAttribute("role", "separator");
      (option.footer ? list : options).append(separator);
    }
    const node = button("", option.label, () => {
      if (trigger.disabled || node.disabled || activePopup?.trigger !== trigger || !list.contains(node)) return;
      if (option.submenu) { openPopupSubmenu(node, option, true); return; }
      // 实例多选菜单允许连续选择；旧菜单节点的迟到点击不能覆盖重绘后的选择状态。
      if (trigger.selectOptions || !option.keepOpen) closePopup(true);
      if (trigger.selectOptions) {
        commitSelection(trigger, option.value);
      } else void option.action();
    }, `popup-option${option.footer ? " popup-option-footer" : ""}${option.danger ? " popup-option-danger" : ""}`);
    node.disabled = !!option.disabled; node.tabIndex = -1;
    node.onfocus = () => {
      if (!activePopup) return;
      activePopup.index = choices.indexOf(option);
      if (activePopup.submenu && activePopup.submenu.value !== node.dataset.value) closePopupSubmenu();
    };
    node.dataset.value = option.value ?? option.label;
    node.setAttribute("role", trigger.selectOptions ? "option" : option.checkable ? "menuitemcheckbox" : "menuitem");
    if (trigger.selectOptions) node.setAttribute("aria-selected", String(trigger.value === option.value));
    else if (option.checkable) node.setAttribute("aria-checked", String(option.selected));
    if (option.renderIcon) node.append(option.renderIcon());
    else if (option.icon) node.append(icon(option.icon));
    const labels = element("span", "popup-option-label");
    labels.append(element("span", "", option.label));
    if (option.description) labels.append(element("small", "muted", option.description));
    node.append(labels);
    if (trigger.selectOptions) node.append(element("span", "choice-check", trigger.value === option.value ? "✓" : ""));
    // 优先事项菜单按用户要求显示“√”；未指定字符的其他勾选菜单保持原有“✓”。
    else if (option.checkable) node.append(element("span", "choice-check", option.selected ? option.checkmark || "✓" : ""));
    if (option.submenu) {
      node.setAttribute("aria-haspopup", "menu"); node.setAttribute("aria-expanded", "false");
      const arrow = icon("chevron"); arrow.classList.add("popup-submenu-arrow"); node.append(arrow);
    }
    node.onpointerenter = event => {
      if (event.pointerType === "touch" || node.disabled || activePopup?.trigger !== trigger) return;
      clearTimeout(activePopup.submenuTimer);
      // 子菜单稍延后展开，允许指针从父项斜向移入子菜单，避免掠过其他父项时立即切走。
      activePopup.submenuTimer = setTimeout(() => {
        if (activePopup?.trigger !== trigger || !list.contains(node)) return;
        if (option.submenu) openPopupSubmenu(node, option);
        else if (activePopup.submenu) closePopupSubmenu();
      }, 120);
    };
    node.onpointerleave = () => { if (activePopup?.trigger === trigger) clearTimeout(activePopup.submenuTimer); };
    (option.footer ? list : options).append(node);
    if (focused === node.dataset.value && !node.disabled) node.focus();
    if (submenu && submenu.value === node.dataset.value) {
      if (node.disabled || submenu.key !== option.submenuKey || !option.submenu) closePopupSubmenu();
      else {
        // 用户要求侧栏刷新不重开弹层；同一父项仅换绑新节点，子菜单内容、焦点及在途读取保持不变。
        submenu.source = node; submenu.option = option;
        node.setAttribute("aria-expanded", "true"); node.setAttribute("aria-controls", submenu.node.id);
      }
    }
  });
  if (submenu && activePopup.submenu === submenu && !list.contains(submenu.source)) closePopupSubmenu();
  if (restoreFocus && !list.contains(document.activeElement)) {
    // 动态环境列表变化时只移动键盘焦点，不改选中值，也不启用已失效的目标。
    (list.querySelector('button[aria-selected="true"]:not(:disabled),button[aria-checked="true"]:not(:disabled)')
      || list.querySelector("button:not(:disabled)"))?.focus();
  }
  fitPopup();
}
export function fitPopup() {
  if (!activePopup) return;
  const { node, trigger, position } = activePopup;
  if (!trigger.isConnected || trigger.hidden || trigger.disabled) { closePopup(); return; }
  const viewport = window.visualViewport;
  const top = viewport?.offsetTop || 0, left = viewport?.offsetLeft || 0;
  const width = viewport?.width || window.innerWidth, height = viewport?.height || window.innerHeight;
  const box = trigger.getBoundingClientRect(), gap = 4, padding = 8;
  const scroller = trigger.closest(".sidebar-content")?.getBoundingClientRect();
  if (box.bottom <= top || box.top >= top + height || box.right <= left || box.left >= left + width
      || (scroller && (box.bottom <= scroller.top || box.top >= scroller.bottom))) { closePopup(); return; }
  // 用户要求 Open in 固定向左：业务菜单注册定位函数后，所有重绘及滚动/缩放重定位均回调它，不能被通用方向规则覆盖。
  if (typeof trigger.popupFit === "function") {
    trigger.popupFit();
    if (activePopup?.submenu) fitPopupSubmenu();
    return;
  }
  const originTop = position?.y ?? box.top;
  const originBottom = position?.y ?? box.bottom + gap;
  const originLeft = position?.x ?? box.left;
  const availableBelow = Math.max(0, top + height - originBottom - padding);
  const availableAbove = Math.max(0, originTop - (position ? 0 : gap) - top - padding);
  const preferredWidth = trigger.popupWidth || (trigger.selectSearchable ? 300 : 200);
  const maxHeight = Math.min(trigger.popupMaxHeight || 360, height * .8);
  if (trigger.popupAutoWidth) {
    // 用户要求类型菜单以旧宽度为下限、随内容自适应；窄屏仍保留两侧各 8px，不改变其他菜单的宽度规则。
    node.style.width = "max-content";
    node.style.minWidth = `${Math.min(preferredWidth, width - padding * 2)}px`;
    node.style.maxWidth = `${width - padding * 2}px`;
  } else {
    node.style.width = `${Math.min(Math.max(box.width, preferredWidth), width - padding * 2)}px`;
  }
  node.style.maxHeight = `${Math.min(maxHeight, Math.max(availableAbove, availableBelow))}px`;
  const menuHeight = node.getBoundingClientRect().height;
  const above = trigger.popupDirection === "up" ? availableAbove > 0
    : availableBelow < menuHeight && availableAbove > availableBelow;
  node.style.maxHeight = `${Math.min(maxHeight, above ? availableAbove : availableBelow)}px`;
  const actualHeight = node.getBoundingClientRect().height;
  const actualWidth = node.getBoundingClientRect().width;
  // 普通 Select、按钮菜单和模型菜单共用水平规则；右键/长按传入的点击坐标继续作为特殊锚点。
  node.style.left = `${position
    ? Math.max(left + padding, Math.min(originLeft, left + width - actualWidth - padding))
    : menuHorizontalLeft(box, actualWidth, { left, width }, padding)}px`;
  node.style.top = `${Math.max(top + padding, Math.min(above ? originTop - (position ? 0 : gap) - actualHeight : originBottom, top + height - padding - actualHeight))}px`;
  node.dataset.direction = above ? "up" : "down";
  if (activePopup.submenu) fitPopupSubmenu();
}

function closePopupSubmenu(restoreFocus = false) {
  const popup = activePopup, submenu = popup?.submenu;
  if (!submenu) return;
  clearTimeout(popup.submenuTimer); popup.submenu = null;
  submenu.controller?.abort();
  submenu.source.setAttribute("aria-expanded", "false");
  submenu.source.removeAttribute("aria-controls");
  if (submenu.node.hasAttribute("popover") && submenu.node.matches(":popover-open")) submenu.node.hidePopover();
  submenu.node.remove(); popup.node.classList.remove("has-submenu");
  if (restoreFocus && submenu.source.isConnected) submenu.source.focus({ preventScroll: true });
}

function openPopupSubmenu(source, option, focus = false) {
  const popup = activePopup;
  if (!popup || source.disabled || !popup.list.contains(source)) return;
  clearTimeout(popup.submenuTimer);
  if (popup.submenu?.value === source.dataset.value) {
    if (focus) {
      popup.submenu.focusWhenLoaded = true;
      popup.submenu.list.querySelector("button:not(:disabled)")?.focus();
    }
    return;
  }
  if (popup.submenu) closePopupSubmenu();
  const node = element("div", `ui-popup popup-submenu${popup.trigger.popupClassName ? ` ${popup.trigger.popupClassName}` : ""}${option.submenuClassName ? ` ${option.submenuClassName}` : ""}`);
  node.id = `${popup.node.id}-submenu`; node.setAttribute("aria-label", option.label);
  const back = button(option.label, `返回，${option.label}`, () => closePopupSubmenu(true), "popup-submenu-back");
  back.prepend(icon("back")); back.hidden = true; back.type = "button";
  const list = element("div", "popup-list");
  list.setAttribute("role", "menu"); list.setAttribute("aria-label", option.label);
  node.append(back, list);
  const submenu = { node, list, back, source, option, value: source.dataset.value,
    key: option.submenuKey, controller: null, focusWhenLoaded: focus };
  popup.submenu = submenu; popup.node.classList.add("has-submenu");
  source.setAttribute("aria-expanded", "true"); source.setAttribute("aria-controls", node.id);
  // 子菜单保留在父菜单 DOM 内供外部点击判断；独立 popover 使其不被父菜单的滚动边界裁切。
  if (typeof node.showPopover === "function") node.setAttribute("popover", "manual");
  popup.node.append(node);
  if (node.hasAttribute("popover")) node.showPopover();
  node.onpointerenter = () => clearTimeout(popup.submenuTimer);
  node.onkeydown = event => {
    if (event.isComposing || activePopup !== popup || popup.submenu !== submenu) return;
    if (["Escape", "ArrowLeft"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation(); closePopupSubmenu(true); return;
    }
    if (event.key === "Tab") { closePopup(true); return; }
    const options = [...list.querySelectorAll("button:not(:disabled)")];
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      const index = options.indexOf(document.activeElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
        : index < 0 ? event.key === "ArrowDown" ? 0 : options.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : options.length - 1)) % options.length;
      options[next]?.focus(); options[next]?.scrollIntoView({ block: "nearest" });
    }
  };
  if (Array.isArray(option.submenu)) renderPopupSubmenu(popup, submenu, option.submenu);
  else void loadPopupSubmenu(popup, submenu);
}

async function loadPopupSubmenu(popup, submenu) {
  submenu.controller?.abort();
  const controller = new AbortController(); submenu.controller = controller;
  submenu.node.setAttribute("aria-busy", "true");
  renderPopupSubmenu(popup, submenu, [{ label: "正在加载打开方式…", disabled: true }]);
  try {
    const choices = await submenu.option.submenu({ signal: controller.signal });
    if (activePopup !== popup || popup.submenu !== submenu || controller.signal.aborted) return;
    renderPopupSubmenu(popup, submenu, choices);
  } catch (error) {
    if (activePopup !== popup || popup.submenu !== submenu || controller.signal.aborted) return;
    renderPopupSubmenu(popup, submenu, [{ label: "重新加载打开方式",
      description: error.message || "读取失败，请重试", keepOpen: true,
      action: () => loadPopupSubmenu(popup, submenu) }]);
  } finally {
    if (submenu.controller === controller) submenu.node.removeAttribute("aria-busy");
  }
}

function renderPopupSubmenu(popup, submenu, choices) {
  if (activePopup !== popup || popup.submenu !== submenu) return;
  submenu.list.replaceChildren();
  for (const option of choices.length ? choices : [{ label: "暂无可用选项", disabled: true }]) {
    const node = button("", option.label, () => {
      if (activePopup !== popup || popup.submenu !== submenu || node.disabled || !submenu.list.contains(node)) return;
      if (!option.keepOpen) closePopup(true);
      void option.action?.();
    }, `popup-option${option.danger ? " popup-option-danger" : ""}`);
    node.type = "button"; node.tabIndex = -1; node.disabled = !!option.disabled;
    node.dataset.value = option.value ?? option.label; node.setAttribute("role", "menuitem");
    if (option.renderIcon) node.append(option.renderIcon());
    else if (option.icon) node.append(icon(option.icon));
    const labels = element("span", "popup-option-label"); labels.append(element("span", "", option.label));
    if (option.description) labels.append(element("small", "muted", option.description));
    node.append(labels); submenu.list.append(node);
  }
  fitPopupSubmenu();
  if (submenu.focusWhenLoaded) {
    const first = submenu.list.querySelector("button:not(:disabled)");
    if (first) { first.focus(); submenu.focusWhenLoaded = false; }
    else { submenu.node.tabIndex = -1; submenu.node.focus({ preventScroll: true }); }
  }
}

function fitPopupSubmenu() {
  const popup = activePopup, submenu = popup?.submenu;
  if (!submenu) return;
  if (!submenu.source.isConnected || submenu.source.disabled) { closePopupSubmenu(); return; }
  const viewport = window.visualViewport;
  const left = viewport?.offsetLeft || 0, top = viewport?.offsetTop || 0;
  const width = viewport?.width || window.innerWidth, height = viewport?.height || window.innerHeight;
  const source = submenu.source.getBoundingClientRect(), parent = popup.node.getBoundingClientRect();
  if (source.bottom <= parent.top || source.top >= parent.bottom) { closePopupSubmenu(); return; }
  // 用户要求会话右键使用两级菜单；子菜单贴父菜单优先向右，空间不足向左，窄屏重叠时提供返回入口。
  // 四边沿用通用弹层的 8px 留白，父子间的 3px 间隔配合悬停延迟，便于移入子菜单。
  const padding = 8, gap = 3, menuWidth = Math.max(0, Math.min(submenu.option.submenuWidth || 200, width - padding * 2));
  const rightSpace = left + width - padding - parent.right - gap;
  const leftSpace = parent.left - gap - left - padding;
  const overlap = Math.max(rightSpace, leftSpace) < menuWidth;
  const rightward = rightSpace >= menuWidth || rightSpace >= leftSpace;
  submenu.back.hidden = !overlap;
  submenu.node.style.width = `${menuWidth}px`;
  submenu.node.style.maxHeight = `${Math.max(0, height - padding * 2)}px`;
  submenu.node.dataset.direction = overlap ? "overlap" : rightward ? "right" : "left";
  const x = overlap ? parent.left : rightward ? parent.right + gap : parent.left - gap - menuWidth;
  const menuHeight = submenu.node.getBoundingClientRect().height;
  submenu.node.style.left = `${Math.max(left + padding, Math.min(x, left + width - menuWidth - padding))}px`;
  submenu.node.style.top = `${Math.max(top + padding, Math.min(source.top - 4, top + height - menuHeight - padding))}px`;
}
