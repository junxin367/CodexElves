const $ = (selector, root = document) => root.querySelector(selector);
const app = $("#app");
document.documentElement.dataset.inputModality = "pointer";
let session = null, sidebar = null, selectedId = null, thread = null, historyCursor = null, historyLoading = false;
let stream = null, connected = false, selectionVersion = 0, busy = false, creating = false, historyLoadingVersion = -1;
let turns = new Map(), collapsed = new Set(), search = "", sidebarTimer = null, refreshTimer = null;
let historyInitialized = false;
let sidebarVersion = 0, approvalSignature = null, pendingTimer = null, streamRetry = null, streamBackoff = 1000;
const expandedProjects = new Set();
const drafts = new Map();
const skillDrafts = new Map();
let selectedSkills = [], selectedModel = "", selectedEffort = "";
let models = [], modelsPromise = null, modelQuery = "", skillChoices = [], skillIndex = 0, skillLoadVersion = 0, skillScope = "";
let skillLoading = null, creationProject = "", threadReadVersion = 0, tokenUsageVersion = 0, notifiedTokenUsage = null;
let recentLimit = 40;
let activePopup = null, sidebarBusy = false;
function validSkillSelection(value) {
  return value == null || (Array.isArray(value) && value.length <= 16 && value.every(skill =>
    skill && typeof skill.name === "string" && skill.name.length > 0 && skill.name.length <= 256
      && typeof skill.path === "string" && skill.path.length > 0 && skill.path.length <= 4096));
}
let pending = null;
try {
  const stored = JSON.parse(sessionStorage.getItem("codex-elves-pending") || "null");
  if (stored) pending = typeof stored.id === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(stored.id)
    && ["send", "create"].includes(stored.action) && typeof stored.payload?.prompt === "string" && validSkillSelection(stored.payload.skills)
    ? stored : { invalid: true };
} catch { pending = { invalid: true }; }
const escapes = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const escape = value => String(value ?? "").replace(/[&<>"']/g, character => escapes[character]);
const randomId = () => globalThis.crypto?.randomUUID?.() || [...crypto.getRandomValues(new Uint8Array(24))].map(byte => byte.toString(16).padStart(2, "0")).join("");
let selectionToken = randomId();
const appearancePresets = {
  light: { background: "#ffffff", foreground: "#1f1f1f", accent: "#171717" },
  dark: { background: "#171717", foreground: "#ededed", accent: "#ededed" }
};
function defaultAppearance(theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") {
  return { theme, fontSize: 14, colors: { light: { ...appearancePresets.light }, dark: { ...appearancePresets.dark } } };
}
function readAppearance() {
  const value = defaultAppearance();
  try {
    const saved = JSON.parse(localStorage.getItem("codex-elves-web-appearance") || "null");
    if (!saved || typeof saved !== "object") return value;
    if (["light", "dark"].includes(saved.theme)) value.theme = saved.theme;
    if (Number.isInteger(saved.fontSize)) value.fontSize = Math.min(20, Math.max(10, saved.fontSize));
    for (const theme of ["light", "dark"]) for (const name of ["background", "foreground", "accent"]) {
      if (/^#[0-9a-f]{6}$/i.test(saved.colors?.[theme]?.[name])) value.colors[theme][name] = saved.colors[theme][name];
    }
  } catch {}
  return value;
}
let appearance = readAppearance();
function mixColor(left, right, weight) {
  return "#" + [1, 3, 5].map(index => Math.round(
    parseInt(left.slice(index, index + 2), 16) * (1 - weight) + parseInt(right.slice(index, index + 2), 16) * weight
  ).toString(16).padStart(2, "0")).join("");
}
function applyAppearance(save = false) {
  const root = document.documentElement;
  const { background, foreground, accent } = appearance.colors[appearance.theme];
  root.dataset.theme = appearance.theme;
  root.style.colorScheme = appearance.theme;
  const brightness = [1, 3, 5].map(index => parseInt(accent.slice(index, index + 2), 16));
  const onAccent = brightness[0] * .299 + brightness[1] * .587 + brightness[2] * .114 > 150 ? "#171717" : "#ffffff";
  const values = {
    bg: background, text: foreground, accent, "on-accent": onAccent,
    side: mixColor(background, foreground, .035), code: mixColor(background, foreground, .05),
    hover: mixColor(background, foreground, .075), line: mixColor(background, foreground, .14),
    muted: mixColor(background, foreground, .58), "font-size": `${appearance.fontSize}px`,
    danger: appearance.theme === "dark" ? "#ff9c97" : "#b42318", green: appearance.theme === "dark" ? "#77c7a0" : "#168350"
  };
  for (const [name, value] of Object.entries(values)) root.style.setProperty(`--${name}`, value);
  $('meta[name="theme-color"]')?.setAttribute("content", background);
  if (save) {
    try { localStorage.setItem("codex-elves-web-appearance", JSON.stringify(appearance)); }
    catch { notice("外观已应用，但浏览器不允许保存；重新打开页面后将恢复默认。", true); }
  }
  refitComposerMenus();
}
applyAppearance();
function openAppearance() {
  closeComposerMenus(); closePopup();
  const trigger = document.activeElement;
  $("#appearance-dialog")?.remove();
  const dialog = element("dialog", "appearance-dialog");
  dialog.id = "appearance-dialog";
  dialog.setAttribute("aria-labelledby", "appearance-title");
  dialog.innerHTML = `<div class="appearance-heading"><h2 id="appearance-title">网页外观</h2><button type="button" class="icon-button" aria-label="关闭外观设置">×</button></div>
    <div class="appearance-body"><p class="muted">仅影响当前浏览器，不修改本机 Codex 的外观。</p>
    <div class="appearance-field"><span>主题</span><div id="appearance-theme-slot"></div></div>
    <div id="appearance-colors"></div>
    <label class="appearance-field">字体大小<span class="appearance-font"><input id="appearance-font" type="range" min="10" max="20" step="1" aria-label="字体大小"><output id="appearance-font-value"></output></span></label></div>
    <div class="appearance-actions"><button type="button" id="appearance-reset">恢复默认</button><button type="button" class="primary" id="appearance-close">完成</button></div>`;
  document.body.append(dialog);
  $("#appearance-theme-slot").append(selectControl("appearance-theme", "网页主题",
    [{ value: "light", label: "浅色" }, { value: "dark", label: "深色" }], appearance.theme));
  const colorFields = [["accent", "强调色"], ["background", "背景色"], ["foreground", "前景色"]];
  const colorPresets = ["#171717", "#3f3f46", "#71717a", "#d4d4d8", "#f4f4f5", "#ffffff", "#2563eb", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#16a34a"];
  const setColor = (name, color) => { appearance.colors[appearance.theme][name] = color; applyAppearance(true); sync(); };
  for (const [name, label] of colorFields) {
    const field = element("div", "color-field");
    const row = element("div", "appearance-field");
    const swatch = element("button", "color-swatch");
    const panel = element("div", "color-panel");
    const presets = element("div", "color-presets");
    const hex = element("input", "color-hex");
    swatch.type = "button"; swatch.id = `appearance-${name}`;
    swatch.setAttribute("aria-label", label); swatch.setAttribute("aria-expanded", "false"); swatch.setAttribute("aria-controls", `appearance-${name}-panel`);
    swatch.onclick = () => {
      const expand = panel.hidden;
      for (const control of $("#appearance-colors").querySelectorAll(".color-swatch")) control.setAttribute("aria-expanded", "false");
      for (const other of $("#appearance-colors").querySelectorAll(".color-panel")) other.hidden = true;
      panel.hidden = !expand; swatch.setAttribute("aria-expanded", String(expand));
    };
    row.append(element("span", "", label), swatch);
    panel.id = `appearance-${name}-panel`; panel.hidden = true;
    for (const color of colorPresets) {
      const preset = button("", `${label} ${color}`, () => setColor(name, color), "color-preset");
      preset.dataset.color = color; preset.style.setProperty("--swatch", color); preset.setAttribute("aria-pressed", "false");
      presets.append(preset);
    }
    hex.id = `appearance-${name}-hex`; hex.type = "text"; hex.maxLength = 7; hex.spellcheck = false; hex.autocomplete = "off"; hex.placeholder = "#rrggbb";
    hex.setAttribute("aria-label", `${label}色值`);
    hex.oninput = () => {
      const valid = /^#[0-9a-f]{6}$/i.test(hex.value);
      hex.setAttribute("aria-invalid", String(!valid));
      if (valid) setColor(name, hex.value.toLowerCase());
    };
    panel.append(presets, hex); field.append(row, panel);
    $("#appearance-colors").append(field);
  }
  const sync = () => {
    setSelectValue($("#appearance-theme"), appearance.theme);
    for (const [name] of colorFields) {
      const color = appearance.colors[appearance.theme][name];
      const hex = $(`#appearance-${name}-hex`);
      $(`#appearance-${name}`).style.setProperty("--swatch", color);
      if (document.activeElement !== hex) { hex.value = color; hex.setAttribute("aria-invalid", "false"); }
      for (const preset of $(`#appearance-${name}-panel .color-presets`).children) preset.setAttribute("aria-pressed", String(preset.dataset.color === color));
    }
    $("#appearance-font").value = appearance.fontSize;
    $("#appearance-font").style.setProperty("--range-fill", `${(appearance.fontSize - 10) * 10}%`);
    $("#appearance-font-value").textContent = `${appearance.fontSize}px`;
  };
  sync();
  $("#appearance-theme").onchange = event => { appearance.theme = event.target.value; applyAppearance(true); sync(); };
  $("#appearance-font").oninput = event => { appearance.fontSize = Number(event.target.value); applyAppearance(true); sync(); };
  $("#appearance-reset").onclick = () => { appearance = defaultAppearance(appearance.theme); applyAppearance(true); sync(); };
  $("#appearance-close").onclick = $(".appearance-heading button", dialog).onclick = () => dialog.close();
  let backdropPressed = false;
  const onBackdrop = event => {
    const box = dialog.getBoundingClientRect();
    return event.target === dialog && (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom);
  };
  dialog.onpointerdown = event => { backdropPressed = onBackdrop(event); };
  dialog.onpointermove = event => { if (!onBackdrop(event)) backdropPressed = false; };
  dialog.onpointercancel = () => { backdropPressed = false; };
  dialog.onpointerup = event => { if (backdropPressed && onBackdrop(event)) dialog.close(); backdropPressed = false; };
  dialog.onclose = () => {
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup();
    document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
    if (trigger?.isConnected && document.activeElement === document.body) trigger.focus();
  };
  document.body.classList.add("modal-open");
  dialog.showModal();
}
const effortLabels = { none: "无", minimal: "最低", low: "低", medium: "中", high: "高", xhigh: "额外高", max: "最高" };
const skillIdentity = skill => `${skill.name}\0${skill.path}`;
const sameSkills = (left, right) => JSON.stringify(left.map(skillIdentity)) === JSON.stringify(right.map(skillIdentity));
function composerPrompt(text = $("#prompt")?.value || "", skills = selectedSkills) {
  const markers = skills.filter(skill => !text.split(/\s+/).includes(`$${skill.name}`)).map(skill => `$${skill.name}`);
  return [...markers, text.trim()].filter(Boolean).join(" ");
}
function snapshotDraft() { return { text: $("#prompt").value, skills: selectedSkills.map(skill => ({ ...skill })) }; }
function draftKey() { return selectedId || `$new:${creationProject}`; }
function rememberDraft() {
  const key = draftKey(), draft = snapshotDraft();
  drafts.set(key, draft.text); skillDrafts.set(key, draft.skills);
}
function restoreDraft(key) {
  $("#prompt").value = drafts.get(key) || "";
  selectedSkills = (skillDrafts.get(key) || []).map(skill => ({ ...skill }));
  renderSelectedSkills();
}
function matchingDraft(draft, payload) {
  return composerPrompt(draft.text, draft.skills) === payload.prompt
    && sameSkills(draft.skills, payload.skills || []);
}
function clearSubmittedDraft(payload, current = true) {
  const key = payload.threadId || `$new:${payload.projectId || ""}`;
  if (matchingDraft({ text: drafts.get(key) || "", skills: skillDrafts.get(key) || [] }, payload)) {
    drafts.delete(key); skillDrafts.delete(key);
  }
  if (current && matchingDraft(snapshotDraft(), payload)) {
    $("#prompt").value = ""; selectedSkills = []; renderSelectedSkills();
  }
}
function resetComposerOptions() {
  selectedModel = selectedEffort = "";
  modelQuery = "";
  skillChoices = []; skillScope = ""; skillLoading = null; ++skillLoadVersion;
  closeComposerMenus();
}
function changeCreationProject() {
  const project = $("#project-select").value;
  if (!creating || project === creationProject) return;
  rememberDraft();
  creationProject = project; ++selectionVersion; selectionToken = randomId();
  restoreDraft(draftKey()); resetComposerOptions(); updateComposer();
}
function selectCreatedThread(id) {
  const key = draftKey(), draft = snapshotDraft();
  drafts.set(id, draft.text); skillDrafts.set(id, draft.skills);
  const selection = selectThread(id);
  drafts.delete(key); skillDrafts.delete(key);
  return selection;
}
function closeSkillMenu() {
  const menu = $("#skill-menu"); if (menu) menu.hidden = true;
  $("#prompt")?.setAttribute("aria-expanded", "false");
  $("#prompt")?.removeAttribute("aria-activedescendant");
}
function closeComposerMenus() {
  const menu = $("#model-settings"); if (menu) menu.hidden = true;
  $("#model-button")?.setAttribute("aria-expanded", "false");
  closeSkillMenu();
}
function tokenCount(value) { return value >= 1000 ? `${Number((value / 1000).toFixed(1))}k` : String(Math.round(value)); }
function contextUsage(info) {
  const total = info?.modelContextWindow, value = info?.last?.totalTokens;
  if (typeof total !== "number" || !Number.isFinite(total) || total <= 0
      || typeof value !== "number" || !Number.isFinite(value) || value < 0) return null;
  const used = Math.min(value, total);
  return { used, total, percent: Math.round(used / total * 100) };
}
async function loadModels(force = false) {
  if (!force && models.length) return models;
  if (modelsPromise) return modelsPromise;
  const deviceId = session?.device.id;
  const request = (async () => {
    const result = await api("/api/read/models", {});
    if (!session || session.device.id !== deviceId) return [];
    models = Array.isArray(result.data) ? result.data : [];
    updateComposer();
    return models;
  })();
  modelsPromise = request;
  try { return await request; } finally { if (modelsPromise === request) modelsPromise = null; }
}
function currentModel() { return models.find(model => model.model === (selectedModel || thread?.model)); }
function renderModelMenu() {
  const menu = $("#model-settings");
  if (!menu) return;
  const focusIndex = [...menu.querySelectorAll("button")].indexOf(document.activeElement);
  const modelScroll = $(".model-options", menu)?.scrollTop || 0;
  const effortScroll = $(".effort-options", menu)?.scrollTop || 0;
  const searchFocused = document.activeElement === $("#model-search");
  const searchSelection = searchFocused ? [document.activeElement.selectionStart, document.activeElement.selectionEnd] : null;
  menu.replaceChildren(element("div", "popover-title", "模型与思考深度"));
  if (models.length > 8) {
    const input = element("input", "model-search");
    input.id = "model-search"; input.type = "search"; input.value = modelQuery;
    input.placeholder = "搜索模型"; input.setAttribute("aria-label", "搜索模型"); input.autocomplete = "off";
    input.oninput = event => { if (!event.isComposing) { modelQuery = input.value; renderModelMenu(); } };
    input.oncompositionend = () => { modelQuery = input.value; renderModelMenu(); };
    menu.append(input);
  }
  const choice = (text, checked, action, description) => {
    const node = button("", text, action, "model-option");
    node.title = text;
    node.setAttribute("role", "menuitemradio"); node.setAttribute("aria-checked", String(checked));
    const labels = element("span", "model-option-label");
    labels.append(element("span", "", text));
    if (description) labels.append(element("small", "muted", description));
    node.append(labels, element("span", "choice-check", checked ? "✓" : ""));
    return node;
  };
  const group = element("div", "model-options"); group.setAttribute("role", "group"); group.setAttribute("aria-label", "模型");
  group.append(choice("继承会话模型", !selectedModel, () => { selectedModel = selectedEffort = ""; renderModelMenu(); updateComposer(); }));
  const visibleModels = models.filter(model => `${model.displayName || ""} ${model.model}`.toLowerCase().includes(modelQuery.trim().toLowerCase()));
  for (const model of visibleModels) group.append(choice(model.displayName || model.model, selectedModel === model.model, () => {
    selectedModel = model.model;
    const efforts = model.supportedReasoningEfforts || [];
    selectedEffort = efforts.find(value => value.reasoningEffort === model.defaultReasoningEffort)?.reasoningEffort
      || efforts.find(value => value.reasoningEffort === "medium")?.reasoningEffort || efforts[0]?.reasoningEffort || "";
    renderModelMenu(); updateComposer();
  }));
  if (!visibleModels.length) group.append(element("p", "popover-empty muted", models.length ? "没有匹配的模型，请修改搜索条件" : "暂无可选模型，可继承会话设置"));
  menu.append(group, element("div", "popover-title", "思考深度"));
  const efforts = element("div", "effort-options"); efforts.setAttribute("role", "group"); efforts.setAttribute("aria-label", "思考深度");
  efforts.append(choice("继承思考深度", !selectedEffort, () => { selectedEffort = ""; renderModelMenu(); updateComposer(); }));
  for (const effort of currentModel()?.supportedReasoningEfforts || []) {
    efforts.append(choice(effortLabels[effort.reasoningEffort] || effort.reasoningEffort,
      selectedEffort === effort.reasoningEffort,
      () => { selectedEffort = effort.reasoningEffort; renderModelMenu(); updateComposer(); closeComposerMenus(); $("#prompt").focus(); },
      effort.description));
  }
  menu.append(efforts);
  fitComposerMenu(menu);
  group.scrollTop = modelScroll; efforts.scrollTop = effortScroll;
  if (searchFocused && $("#model-search")) {
    $("#model-search").focus({ preventScroll: true });
    $("#model-search").setSelectionRange(...searchSelection);
  } else if (focusIndex >= 0) menu.querySelectorAll("button")[focusIndex]?.focus();
}
function fitComposerMenu(menu) {
  if (menu.id === "model-settings") {
    const anchor = $("#model-button");
    if (!anchor || menu.hidden) return;
    const viewport = window.visualViewport;
    const top = viewport?.offsetTop || 0, left = viewport?.offsetLeft || 0;
    const width = viewport?.width || window.innerWidth, height = viewport?.height || window.innerHeight;
    const box = anchor.getBoundingClientRect(), gap = 4, padding = 8;
    const above = Math.max(0, box.top - top - gap - padding);
    const below = Math.max(0, top + height - box.bottom - gap - padding);
    const limit = Math.min(560, Math.max(above, below));
    menu.style.width = "max-content";
    menu.style.minWidth = `${Math.min(300, width - padding * 2)}px`;
    menu.style.maxWidth = `${Math.min(350, width - padding * 2)}px`;
    menu.style.maxHeight = `${limit}px`;
    $(".effort-options", menu)?.style.setProperty("max-height", `${Math.max(0, (limit - 80) * .45)}px`);
    const upwards = below < menu.getBoundingClientRect().height && above > below;
    const available = Math.min(560, upwards ? above : below);
    menu.style.maxHeight = `${available}px`;
    $(".effort-options", menu)?.style.setProperty("max-height", `${Math.max(0, (available - 80) * .45)}px`);
    const actual = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(left + padding, Math.min(box.left, left + width - actual.width - padding))}px`;
    menu.style.top = `${upwards ? box.top - gap - actual.height : box.bottom + gap}px`;
    menu.dataset.direction = upwards ? "up" : "down";
    return;
  }
  const top = $("#composer")?.getBoundingClientRect().top || 300;
  menu.style.maxHeight = `${Math.max(100, top - 16)}px`;
}
function refitComposerMenus() {
  fitPopup();
  for (const id of ["model-settings", "skill-menu"]) {
    const menu = $(`#${id}`); if (menu && !menu.hidden) fitComposerMenu(menu);
  }
}
window.addEventListener("resize", refitComposerMenus);
window.visualViewport?.addEventListener("resize", refitComposerMenus);
window.visualViewport?.addEventListener("scroll", refitComposerMenus);
document.addEventListener("scroll", event => {
  if (activePopup && !activePopup.node.contains(event.target)) fitPopup();
  const menu = $("#model-settings");
  if (menu && !menu.hidden && !menu.contains(event.target)) fitComposerMenu(menu);
}, true);
function renderSelectedSkills() {
  const node = $("#selected-skills");
  if (!node) return;
  node.replaceChildren(); node.hidden = !selectedSkills.length;
  for (const skill of selectedSkills) {
    const chip = element("span", "skill-chip");
    chip.append(element("span", "skill-chip-name", `✦ ${skill.displayName || skill.name}`));
    const remove = button("×", `移除 Skill ${skill.displayName || skill.name}`, () => {
      selectedSkills = selectedSkills.filter(item => skillIdentity(item) !== skillIdentity(skill));
      renderSelectedSkills(); $("#prompt").focus();
    }, "skill-remove");
    remove.disabled = session?.readOnly;
    chip.append(remove); node.append(chip);
  }
  updateComposer();
}
function slashQuery() {
  const prompt = $("#prompt");
  if (!prompt || prompt.selectionStart !== prompt.selectionEnd) return null;
  const text = prompt.value.slice(0, prompt.selectionStart);
  const match = /(?:^|\s)\/([^\s/]*)$/.exec(text);
  return match ? { query: match[1].toLowerCase(), start: prompt.selectionStart - match[1].length - 1, end: prompt.selectionStart } : null;
}
function renderSkillMenu(message) {
  const menu = $("#skill-menu"), query = slashQuery();
  if (!menu || !query || session?.readOnly) { closeSkillMenu(); return; }
  closePopup();
  menu.hidden = false; $("#model-settings").hidden = true; $("#model-button").setAttribute("aria-expanded", "false");
  $("#prompt").setAttribute("aria-expanded", "true");
  menu.replaceChildren(element("div", "popover-title", "选择 Skill"));
  const matches = skillChoices.filter(skill => !selectedSkills.some(item => skillIdentity(item) === skillIdentity(skill))
    && [skill.name, skill.displayName, skill.description].some(value => String(value).toLowerCase().includes(query.query)));
  skillIndex = Math.min(Math.max(0, skillIndex), Math.max(0, matches.length - 1));
  if (message || !matches.length) menu.append(element("p", "popover-empty muted", message || "没有匹配的可用 Skill"));
  matches.forEach((skill, index) => {
    const node = button("", skill.displayName || skill.name, () => selectSkill(skill), `skill-option${index === skillIndex ? " active" : ""}`);
    node.id = `skill-option-${index}`; node.setAttribute("role", "option"); node.setAttribute("aria-selected", String(index === skillIndex));
    node.append(element("strong", "", skill.displayName || skill.name), element("small", "muted", skill.description));
    node.onmousedown = event => event.preventDefault();
    menu.append(node);
  });
  if (matches.length) {
    $("#prompt").setAttribute("aria-activedescendant", `skill-option-${skillIndex}`);
    $(`#skill-option-${skillIndex}`)?.scrollIntoView({ block: "nearest" });
  } else $("#prompt").removeAttribute("aria-activedescendant");
  fitComposerMenu(menu);
}
async function updateSkillMenu() {
  const query = slashQuery(), prompt = $("#prompt");
  if (!query || !session || session.readOnly || document.activeElement !== prompt) { closeSkillMenu(); return; }
  const projectId = creating ? $("#project-select").value : "";
  const scope = selectedId ? `thread:${selectedId}` : projectId ? `project:${projectId}` : "";
  if (!scope) { renderSkillMenu("请先选择会话，或为新会话选择项目"); return; }
  if (skillScope === scope) { renderSkillMenu(); return; }
  if (skillLoading?.scope === scope) { renderSkillMenu("正在读取本机可用 Skill…"); return; }
  const version = ++skillLoadVersion, selection = selectionVersion, deviceId = session.device.id;
  const loading = skillLoading = { scope };
  skillChoices = [];
  renderSkillMenu("正在读取本机可用 Skill…");
  try {
    const result = await api("/api/read/skills", selectedId ? { threadId: selectedId } : { projectId });
    if (version !== skillLoadVersion || selection !== selectionVersion || session?.device.id !== deviceId) return;
    skillChoices = Array.isArray(result.data) ? result.data : []; skillScope = scope; skillIndex = 0;
    if ($("#skill-menu") && !$("#skill-menu").hidden && document.activeElement === $("#prompt")) renderSkillMenu();
  } catch (error) {
    if (version === skillLoadVersion && selection === selectionVersion && $("#skill-menu") && !$("#skill-menu").hidden) renderSkillMenu(error.message);
  } finally { if (skillLoading === loading) skillLoading = null; }
}
function selectSkill(skill) {
  const query = slashQuery();
  if (!query || selectedSkills.length >= 16) { if (selectedSkills.length >= 16) notice("最多选择 16 项 Skill", true); return; }
  const prompt = $("#prompt");
  prompt.setRangeText("", query.start, query.end, "end");
  selectedSkills.push({ ...skill });
  renderSelectedSkills(); closeComposerMenus(); prompt.focus();
}
document.addEventListener("pointerdown", event => {
  document.documentElement.dataset.inputModality = "pointer";
  if (activePopup && !activePopup.node.contains(event.target) && !activePopup.trigger.contains(event.target)) closePopup();
  if (!event.target.closest("#model-button,#model-settings,#skill-menu,#prompt")) closeComposerMenus();
});
document.addEventListener("focusin", event => {
  if (activePopup && !activePopup.node.contains(event.target) && !activePopup.trigger.contains(event.target)) closePopup();
});
document.addEventListener("keydown", event => {
  if (event.key === "Tab") document.documentElement.dataset.inputModality = "keyboard";
  if (event.key === "Escape") {
    if (activePopup) { event.preventDefault(); event.stopPropagation(); closePopup(true); return; }
    const modelOpen = $("#model-settings") && !$("#model-settings").hidden;
    closeComposerMenus();
    if (modelOpen) $("#model-button").focus();
  }
});
const element = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
};
function button(text, label, action, className = "") {
  const node = element("button", className, text);
  node.type = "button";
  node.title = label;
  node.setAttribute("aria-label", label);
  node.addEventListener("click", action);
  return node;
}
function icon(name) {
  const paths = {
    chevron: '<path d="m6 9 6 6 6-6"/>', folder: '<path d="M3 9V5a2 2 0 0 1 2-2h4l2 3h8a2 2 0 0 1 2 2v2"/><path d="M3 10h18l-3 10H3a1 1 0 0 1-1-1Z"/>',
    chat: '<path d="M20 11.5a8.5 8.5 0 0 1-8.5 8.5 9 9 0 0 1-4-.9L3 21l1.9-4.5A9 9 0 0 1 4 12.5 8.5 8.5 0 0 1 12.5 4h.5a8.5 8.5 0 0 1 7 7.5Z"/>',
    more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    pin: '<path d="m16 3 5 5-4 1-3 5-4-4 5-3Z"/><path d="m10 14-7 7"/>',
    plus: '<path d="M12 5v14M5 12h14"/>', check: '<path d="m5 12 4 4L19 6"/>',
    settings: '<path d="m9 3-.5 3-2 1.2-2.8-1-3 5 2.3 2-.1 2.3-2.2 2 3 5 2.8-1L9 22l.5 3h5l.5-3 2.5-1.5 2.8 1 3-5-2.2-2-.1-2.3 2.3-2-3-5-2.8 1-2-1.2L14 3Z" transform="translate(1 -1) scale(.85)"/><circle cx="12" cy="12" r="3"/>',
    logout: '<path d="M9 4H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h4M9 12h12m-4-4 4 4-4 4"/>',
    warning: '<path d="m12 3 10 18H2Z"/><path d="M12 9v5m0 3h.01"/>',
    arrowUp: '<path d="M12 19V5m-6 6 6-6 6 6"/>'
  };
  const node = element("span", "ui-icon");
  node.setAttribute("aria-hidden", "true");
  node.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${paths[name] || ""}</svg>`;
  return node;
}
function selectControl(id, label, options, value, className = "") {
  const node = button("", label, () => togglePopup(node), `select-trigger ${className}`.trim());
  node.id = id; node.setAttribute("aria-haspopup", "listbox"); node.setAttribute("aria-expanded", "false");
  node.selectOptions = options;
  setSelectValue(node, value);
  node.onkeydown = event => {
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault(); openPopup(node, { edge: event.key === "Home" ? "first" : event.key === "End" ? "last" : null });
    }
  };
  return node;
}
function setSelectValue(node, value) {
  const selected = node.selectOptions.find(option => option.value === value) || node.selectOptions[0];
  node.value = selected?.value || "";
  node.setAttribute("aria-description", `当前选择：${selected?.label || "暂无可选项目"}`);
  node.title = `${node.getAttribute("aria-label")}：${selected?.label || "暂无可选项目"}${selected?.description ? ` · ${selected.description}` : ""}`;
  node.replaceChildren(element("span", "select-label", selected?.label || "暂无可选项目"), icon("chevron"));
  if (activePopup?.trigger === node) renderPopup();
}
function setSelectOptions(node, options, value = node.value) {
  node.selectOptions = options;
  setSelectValue(node, value);
}
function closePopup(restoreFocus = false) {
  if (!activePopup) return;
  const { node, trigger } = activePopup;
  activePopup = null;
  trigger.setAttribute("aria-expanded", "false");
  trigger.removeAttribute("aria-controls");
  if (node.hasAttribute("popover") && node.matches(":popover-open")) node.hidePopover();
  node.remove();
  if (restoreFocus && trigger.isConnected && !trigger.disabled) trigger.focus();
}
function togglePopup(trigger) {
  if (activePopup?.trigger === trigger) closePopup(true);
  else openPopup(trigger);
}
function openPopup(trigger, { query = "", edge = null, focus = true, position = null } = {}) {
  if (trigger.disabled || !trigger.isConnected) return;
  closePopup(); closeComposerMenus();
  const node = element("div", "ui-popup");
  node.id = "active-popup";
  const selecting = !!trigger.selectOptions;
  node.setAttribute("aria-label", trigger.getAttribute("aria-label"));
  const list = element("div", "popup-list");
  list.setAttribute("role", selecting ? "listbox" : "menu");
  list.setAttribute("aria-label", trigger.getAttribute("aria-label"));
  let input = null;
  if (trigger.selectSearchable) {
    input = element("input", "popup-search");
    input.type = "search"; input.placeholder = "搜索项目名称或路径"; input.autocomplete = "off";
    input.setAttribute("aria-label", "搜索可选项目"); input.value = query;
    input.oninput = () => { if (activePopup) { activePopup.query = input.value; activePopup.index = 0; renderPopup(); } };
    node.append(input);
  }
  node.append(list);
  const choices = selecting ? trigger.selectOptions : trigger.menuOptions;
  activePopup = { node, trigger, list, input, query, position, index: Math.max(0, choices.findIndex(option => option.value === trigger.value)), choices: [] };
  trigger.setAttribute("aria-expanded", "true"); trigger.setAttribute("aria-controls", node.id);
  const host = trigger.closest("dialog") || document.body;
  if (typeof node.showPopover === "function") node.setAttribute("popover", "manual");
  host.append(node);
  renderPopup();
  if (activePopup?.node !== node) return;
  if (node.hasAttribute("popover")) node.showPopover();
  if (edge) activePopup.index = edge === "first" ? 0 : activePopup.choices.length - 1;
  if (focus) (input && !edge ? input : list.querySelectorAll("button")[activePopup.index])?.focus();
  fitPopup();
  node.onkeydown = event => {
    if (event.isComposing || !activePopup) return;
    const options = [...list.querySelectorAll("button:not(:disabled)")];
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) && !(event.target === input && ["Home", "End"].includes(event.key))) {
      event.preventDefault();
      const index = options.indexOf(document.activeElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
        : index < 0 ? event.key === "ArrowDown" ? 0 : options.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : options.length - 1)) % options.length;
      activePopup.index = next;
      options[next]?.focus(); options[next]?.scrollIntoView({ block: "nearest" });
    } else if (event.target === input && event.key === "Enter") {
      event.preventDefault(); options[Math.min(activePopup.index, options.length - 1)]?.click();
    } else if (event.key === "Tab") closePopup(true);
    else if (event.target !== input && event.key.length === 1 && !event.ctrlKey && !event.metaKey && event.key !== " ") {
      const start = options.indexOf(document.activeElement) + 1;
      const ordered = [...options.slice(start), ...options.slice(0, start)];
      ordered.find(option => option.textContent.trim().toLowerCase().startsWith(event.key.toLowerCase()))?.focus();
    }
  };
}
function renderPopup() {
  if (!activePopup) return;
  const { trigger, list, query } = activePopup;
  const focused = list.contains(document.activeElement) ? document.activeElement.dataset.value : null;
  const choices = (trigger.selectOptions || trigger.menuOptions).filter(option =>
    !query || `${option.label} ${option.description || ""}`.toLowerCase().includes(query.trim().toLowerCase()));
  activePopup.choices = choices;
  list.replaceChildren();
  if (!choices.length) list.append(element("p", "popover-empty muted", query ? "没有匹配的项目，请修改搜索条件" : "暂无可用选项"));
  choices.forEach(option => {
    const node = button("", option.label, () => {
      if (trigger.disabled || node.disabled) return;
      closePopup(true);
      if (trigger.selectOptions) {
        if (trigger.value === option.value) return;
        setSelectValue(trigger, option.value);
        trigger.dispatchEvent(new Event("change", { bubbles: true }));
      } else void option.action();
    }, "popup-option");
    node.disabled = !!option.disabled; node.tabIndex = -1;
    node.onfocus = () => { if (activePopup) activePopup.index = choices.indexOf(option); };
    node.dataset.value = option.value ?? option.label;
    node.setAttribute("role", trigger.selectOptions ? "option" : "menuitem");
    if (trigger.selectOptions) node.setAttribute("aria-selected", String(trigger.value === option.value));
    if (option.icon) node.append(icon(option.icon));
    const labels = element("span", "popup-option-label");
    labels.append(element("span", "", option.label));
    if (option.description) labels.append(element("small", "muted", option.description));
    node.append(labels);
    if (trigger.selectOptions) node.append(element("span", "choice-check", trigger.value === option.value ? "✓" : ""));
    list.append(node);
    if (focused === node.dataset.value) node.focus();
  });
  fitPopup();
}
function fitPopup() {
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
  const originTop = position?.y ?? box.top;
  const originBottom = position?.y ?? box.bottom + gap;
  const originLeft = position?.x ?? box.left;
  const availableBelow = Math.max(0, top + height - originBottom - padding);
  const availableAbove = Math.max(0, originTop - (position ? 0 : gap) - top - padding);
  node.style.width = `${Math.min(Math.max(box.width, trigger.selectSearchable ? 300 : 200), width - padding * 2)}px`;
  node.style.maxHeight = `${Math.min(360, Math.max(availableAbove, availableBelow))}px`;
  const menuHeight = node.getBoundingClientRect().height;
  const above = availableBelow < menuHeight && availableAbove > availableBelow;
  node.style.maxHeight = `${Math.min(360, above ? availableAbove : availableBelow)}px`;
  const actualHeight = node.getBoundingClientRect().height;
  node.style.left = `${Math.max(left + padding, Math.min(originLeft, left + width - node.getBoundingClientRect().width - padding))}px`;
  node.style.top = `${Math.max(top + padding, Math.min(above ? originTop - (position ? 0 : gap) - actualHeight : originBottom, top + height - padding - actualHeight))}px`;
  node.dataset.direction = above ? "up" : "down";
}
function rowMenu(id, label, options) {
  const node = button("", label, () => togglePopup(node), "row-menu-button");
  node.id = id; node.append(icon("more")); node.menuOptions = options;
  node.setAttribute("aria-haspopup", "menu"); node.setAttribute("aria-expanded", "false");
  node.disabled = sidebarBusy || !connected;
  node.onkeydown = event => {
    if (["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); openPopup(node, { edge: event.key === "ArrowUp" ? "last" : "first" }); }
  };
  return node;
}
async function changeSidebar(payload) {
  if (!session || session.readOnly || sidebarBusy || !connected) return;
  const deviceId = session.device.id;
  const focus = $("#sidebar-content").contains(document.activeElement) ? document.activeElement : null;
  const focusedThread = focus?.closest(".thread-row")?.dataset.threadId;
  sidebarBusy = true; renderSidebar();
  try { await mutate(payload.threadId ? "pinThread" : payload.projectId ? "pinProject" : "sort", payload); await loadSidebar(); }
  catch (error) { notice(error.message, true); }
  finally {
    if (session?.device.id === deviceId) {
      sidebarBusy = false; renderSidebar();
      if (focus && document.activeElement === document.body) {
        const restored = document.getElementById(focus.id) || [...$("#sidebar-content").querySelectorAll(".thread-row")]
          .find(item => item.dataset.threadId === focusedThread)?.querySelector(focus.classList.contains("row-menu-button") ? ".row-menu-button" : ".thread-link");
        restored?.focus({ preventScroll: true });
      }
    }
  }
}
function confirmAction(title, message, action, danger = false) {
  closePopup(); closeComposerMenus();
  if ($("#confirmation-dialog")) return Promise.resolve(false);
  const trigger = document.activeElement;
  const dialog = element("dialog", "confirmation-dialog");
  dialog.id = "confirmation-dialog";
  dialog.setAttribute("aria-labelledby", "confirmation-title"); dialog.setAttribute("aria-describedby", "confirmation-message");
  const heading = element("h2", "", title); heading.id = "confirmation-title";
  const description = element("p", "muted", message); description.id = "confirmation-message";
  const actions = element("div", "confirmation-actions");
  actions.append(button("取消", "取消操作", () => dialog.close("cancel")),
    button(action, action, () => dialog.close("confirm"), danger ? "danger-button" : "primary"));
  const content = element("div", "confirmation-content"), text = element("div");
  text.append(heading, description); content.append(icon("warning"), text);
  dialog.append(content, actions); document.body.append(dialog);
  return new Promise(resolve => {
    dialog.onclose = () => {
      const confirmed = dialog.returnValue === "confirm";
      dialog.remove(); document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
      if (trigger?.isConnected && document.activeElement === document.body) trigger.focus();
      resolve(confirmed);
    };
    document.body.classList.add("modal-open");
    dialog.showModal(); $("button", actions).focus();
  });
}
function notice(message, error = false) {
  const node = $("#notice");
  node.textContent = message;
  node.className = `visible${error ? " error" : ""}`;
  if (typeof node.showPopover === "function") {
    node.setAttribute("popover", "manual");
    if (!node.matches(":popover-open")) node.showPopover();
  }
  clearTimeout(notice.timer);
  notice.timer = setTimeout(() => {
    node.className = "";
    if (node.hasAttribute("popover") && node.matches(":popover-open")) node.hidePopover();
  }, 6500);
}
async function api(path, body, id) {
  const deviceId = session?.device.id;
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST", credentials: "same-origin",
    headers: { ...(body !== undefined ? { "Content-Type": "application/json" } : {}), ...(id ? { "Idempotency-Key": id } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body)
  });
  const result = await response.json().catch(() => ({ message: `HTTP ${response.status}` }));
  if (!response.ok) {
    if (response.status === 401 && !path.endsWith("/connect") && deviceId === session?.device.id) { disconnect(); showLogin(); }
    throw Object.assign(new Error(result.message || "请求失败"), { code: response.status });
  }
  return result;
}
function savePending(value) {
  if (value) {
    try { sessionStorage.setItem("codex-elves-pending", JSON.stringify(value)); }
    catch { throw new Error("无法保存发送回执，消息未提交。请允许此网页使用浏览器会话存储。"); }
  } else {
    try { sessionStorage.removeItem("codex-elves-pending"); }
    catch { notice("无法清理浏览器回执记录，重新打开页面后可能仍需人工核对。", true); }
  }
  clearTimeout(pendingTimer);
  pending = value;
}
async function mutate(action, payload, { reliable = false } = {}) {
  const version = selectionVersion;
  const id = reliable && pending ? pending.id : randomId();
  if (reliable && !pending) savePending({ id, action, payload, deviceId: session.device.id, selectionToken, time: Date.now() });
  const ownsPending = () => reliable && pending?.id === id;
  let result;
  try { result = await api(`/api/action/${action}`, payload, id); }
  catch (error) {
    if (reliable) {
      // A network failure does not prove the native client rejected the message.
      if (error.code && error.code !== 503 && ownsPending()) savePending(null);
      else notice("未收到回执，正在确认送达状态。请勿重复发送。", true);
    }
    throw error;
  }
  if (result.state === "pending") {
    if (ownsPending()) void resolvePending();
    throw new Error("操作仍在处理，请稍后查看会话或回执");
  }
  if (result.state === "unknown") throw new Error(result.result?.message || "送达状态未知，请在会话中确认，勿重复发送");
  if (ownsPending()) savePending(null);
  if (result.state === "failed") {
    if (action === "create" && result.result?.threadId && creating && selectionVersion === version) {
      await selectCreatedThread(result.result.threadId);
    }
    throw new Error(result.result?.message || "原生操作未完成");
  }
  return result.result;
}
async function resolvePending() {
  if (!pending || !session) return;
  const current = pending;
  const deviceId = session.device.id;
  const version = selectionVersion;
  if (current.invalid || (current.deviceId && current.deviceId !== deviceId)) {
    notice("回执记录损坏或属于旧设备授权。请先检查原生会话，再人工解除锁定；不会自动重发。", true);
    updateComposer(); return;
  }
  const stillCurrent = () => pending === current && session?.device.id === deviceId;
  const retry = delay => {
    clearTimeout(pendingTimer);
    pendingTimer = setTimeout(() => { if (stillCurrent()) void resolvePending(); }, delay);
  };
  try {
    const receipt = await api(`/api/receipts/${current.id}`);
    if (!stillCurrent()) return;
    if (receipt.state === "pending") { retry(2000); return; }
    if (receipt.state === "unknown") { notice("上一条消息送达状态未知。请查看会话，确认后再解除发送锁定。", true); updateComposer(); return; }
    savePending(null);
    if (receipt.state === "completed") {
      clearSubmittedDraft(current.payload,
        (selectedId === current.payload.threadId || (creating && current.action === "create")) && current.selectionToken === selectionToken);
      notice("上一条消息已确认送达");
    } else notice(receipt.result?.message || "上一条消息未发送", true);
    if (current.action === "create" && receipt.result?.threadId && creating
        && current.selectionToken === selectionToken && version === selectionVersion) await selectCreatedThread(receipt.result.threadId);
    else if (selectedId === current.payload.threadId) await reloadThread();
    updateComposer();
  } catch (error) {
    if (!stillCurrent()) return;
    if (error.code === 404) {
      notice("未找到上一条消息的回执。不会自动重发；请先检查会话。", true);
      updateComposer();
    } else retry(5000);
  }
}
function showLogin() {
  closePopup();
  session = null;
  sidebar = null; selectedId = null; thread = null; creating = false;
  historyCursor = null; historyInitialized = false; approvalSignature = null;
  turns.clear(); drafts.clear(); skillDrafts.clear(); selectedSkills = []; selectedModel = selectedEffort = "";
  models = []; modelsPromise = null; skillScope = ""; skillChoices = []; skillLoading = null; ++skillLoadVersion; selectionToken = randomId();
  creationProject = ""; notifiedTokenUsage = null; ++threadReadVersion;
  sidebarBusy = false;
  $("#confirmation-dialog")?.close("cancel");
  $("#appearance-dialog")?.close();
  app.replaceChildren();
  const panel = element("main", "pair-panel");
  panel.append(element("div", "brand-mark", "✦"), element("h1", "", "连接 CodexElves"), element("p", "muted", "输入管理器中设置的访问密钥，进入本机 Codex 会话。"));
  const form = element("form", "pair-form");
  form.noValidate = true;
  const key = element("input");
  key.type = "password"; key.placeholder = "输入访问密钥"; key.required = true;
  key.autocomplete = "off"; key.minLength = 32; key.maxLength = 256; key.spellcheck = false;
  key.setAttribute("aria-label", "访问密钥");
  const hashParameters = new URLSearchParams(location.hash.slice(1));
  if (hashParameters.has("pair") || hashParameters.has("key")) history.replaceState(null, "", location.pathname);
  const submit = element("button", "primary", "连接"); submit.type = "submit";
  form.append(key, submit);
  const errorText = element("p", "error-text"); errorText.setAttribute("role", "alert");
  form.addEventListener("submit", async event => {
    event.preventDefault();
    const value = key.value.trim();
    if (!/^[\x21-\x7e]{32,256}$/.test(value)) { errorText.textContent = "请填写 32–256 个非空格 ASCII 字符的访问密钥"; key.focus(); return; }
    submit.disabled = true; errorText.textContent = "";
    try {
      await api("/api/connect", { key: value });
      key.value = ""; await connect();
    } catch (error) { errorText.textContent = error.message; }
    finally { submit.disabled = false; }
  });
  panel.append(form, errorText, element("small", "muted", "请勿向他人透露密钥。会话与桌面共用，退出或授权到期后可重新输入密钥连接。"),
    button("⚙ 网页外观", "网页外观设置", openAppearance, "pair-appearance-button text-button"));
  app.append(panel);
}
function shell() {
  approvalSignature = null;
  app.innerHTML = `<div class="workspace">
    <div class="sidebar-backdrop"></div>
    <aside class="sidebar" aria-label="会话侧边栏">
      <div class="sidebar-header"><strong>✦ CodexElves</strong><button id="close-sidebar" class="icon-button" aria-label="关闭侧边栏">×</button></div>
      <button id="new-chat" class="new-chat" type="button">＋ 新建会话</button>
      <input id="sidebar-search" class="search" placeholder="搜索会话与项目" aria-label="搜索会话与项目">
      <div id="sidebar-controls" class="sidebar-controls"></div>
      <div id="sidebar-content" class="sidebar-content"></div>
      <div class="sidebar-footer"><div class="footer-actions"><button id="appearance-button" class="footer-button" type="button" aria-label="网页外观设置"></button><button id="logout" class="footer-button" type="button"></button></div></div>
    </aside>
    <main class="conversation">
      <header class="conversation-header"><button id="open-sidebar" class="icon-button" aria-label="打开侧边栏">☰</button><div class="thread-heading"><strong id="thread-title">远程会话</strong><span id="thread-path" class="muted"></span></div><span id="connection-state" class="connection-state" role="status">连接中</span><button id="refresh" class="icon-button" aria-label="刷新会话">↻</button></header>
      <div id="messages" class="messages" role="log" aria-label="会话消息"></div>
      <div id="approval-area" class="approval-area" aria-live="polite"></div>
      <div class="composer-area"><div id="pending-area"></div><form id="composer" class="composer">
        <div id="project-select-slot" hidden></div>
        <div id="selected-skills" class="selected-skills" aria-label="已选择的 Skill" hidden></div>
        <textarea id="prompt" rows="3" placeholder="给 Codex 发送消息，输入 / 选择 Skill…" aria-label="消息输入" aria-autocomplete="list" aria-controls="skill-menu" aria-expanded="false"></textarea>
        <div class="composer-toolbar"><div class="composer-controls"><span id="context-info" class="context-info muted"></span><button id="model-button" class="text-button model-button" type="button" aria-label="选择模型与思考深度" aria-haspopup="menu" aria-expanded="false">继承原生模型</button></div><div class="composer-send"><button id="stop" class="stop-button" type="button" hidden>■ 停止</button><button id="send" class="send-button" type="submit" aria-label="发送消息"></button></div></div>
        <div id="model-settings" class="composer-popover" role="menu" aria-label="模型与思考深度" hidden></div>
        <div id="skill-menu" class="composer-popover skill-menu" role="listbox" aria-label="Skill 选择" hidden></div>
      </form></div>
    </main>
  </div>`;
  $("#messages").append(welcomeNode());
  $("#sidebar-controls").append(
    selectControl("grouping", "侧边栏分组", [{ value: "project", label: "按项目" }, { value: "list", label: "最近会话" }], "project"),
    selectControl("sorting", "会话排序", [{ value: "updated_at", label: "最近更新" }, { value: "manual", label: "手动排序" }], "updated_at"));
  const projectSelect = selectControl("project-select", "新会话项目", [], "");
  projectSelect.selectSearchable = true;
  $("#project-select-slot").append(projectSelect);
  $("#send").append(icon("arrowUp"));
  $("#logout").append(icon("logout"), element("span", "", "退出"));
  $("#appearance-button").append(icon("settings"), element("span", "", "设置"));
  $("#appearance-button").title = "网页外观设置";
  $("#open-sidebar").onclick = () => $(".workspace").classList.add("sidebar-open");
  const closeSidebar = () => { closePopup(); $(".workspace").classList.remove("sidebar-open"); };
  $("#close-sidebar").onclick = closeSidebar; $(".sidebar-backdrop").onclick = closeSidebar;
  $("#sidebar-search").oninput = event => { search = event.target.value.trim().toLowerCase(); renderSidebar(); };
  $("#new-chat").onclick = () => startNewChat();
  $("#logout").onclick = async () => {
    const deviceId = session.device.id;
    if (!await confirmAction("退出", "将撤销当前浏览器的访问授权；再次进入需要重新输入访问密钥。", "退出", true) || session?.device.id !== deviceId) return;
    $("#logout").disabled = true;
    try { await api("/api/logout", {}); disconnect(); savePending(null); showLogin(); }
    catch (error) { notice(error.message, true); if ($("#logout")) $("#logout").disabled = false; }
  };
  $("#refresh").onclick = () => void refreshAll().catch(error => notice(error.message, true));
  $("#appearance-button").onclick = openAppearance;
  $("#grouping").onchange = event => void changeSidebar({ target: "grouping", mode: "updated_at", grouping: event.target.value });
  $("#sorting").onchange = event => void changeSidebar({
    target: sidebar.preferences.mode === "project" ? "projects" : "chats", mode: event.target.value
  });
  $("#composer").onsubmit = async event => { event.preventDefault(); await sendMessage(); };
  $("#prompt").onkeydown = event => {
    if (event.isComposing) return;
    if (!$("#skill-menu").hidden && ["ArrowDown", "ArrowUp", "Enter", "Tab"].includes(event.key) && !event.ctrlKey && !event.metaKey) {
      const query = slashQuery();
      const values = skillChoices.filter(skill => !selectedSkills.some(item => skillIdentity(item) === skillIdentity(skill))
        && [skill.name, skill.displayName, skill.description].some(value => String(value).toLowerCase().includes(query?.query || "")));
      if (values.length) {
        event.preventDefault();
        if (event.key === "ArrowDown") { skillIndex = (skillIndex + 1) % values.length; renderSkillMenu(); }
        else if (event.key === "ArrowUp") { skillIndex = (skillIndex + values.length - 1) % values.length; renderSkillMenu(); }
        else selectSkill(values[skillIndex]);
        return;
      }
    }
    if (event.key === "Backspace" && !$("#prompt").value && selectedSkills.length) {
      selectedSkills.pop(); renderSelectedSkills(); event.preventDefault(); return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !event.isComposing) {
      event.preventDefault(); void sendMessage();
    }
  };
  $("#prompt").oninput = event => {
    updateComposer();
    if (event.isComposing) { closeSkillMenu(); return; }
    skillIndex = 0; void updateSkillMenu();
  };
  $("#prompt").oncompositionend = () => { updateComposer(); skillIndex = 0; void updateSkillMenu(); };
  $("#prompt").onkeyup = event => {
    if (!event.isComposing && ["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) void updateSkillMenu();
  };
  $("#prompt").onselect = () => void updateSkillMenu();
  $("#prompt").onblur = event => { if (!event.relatedTarget?.closest("#skill-menu")) closeSkillMenu(); };
  $("#prompt").onclick = () => void updateSkillMenu();
  $("#project-select").onchange = changeCreationProject;
  $("#stop").onclick = async () => {
    try { await mutate("stop", { threadId: selectedId }); notice("已请求停止并清理此会话的后台终端"); await reloadThread(); }
    catch (error) { notice(error.message, true); }
  };
  $("#model-button").onclick = async () => {
    const opening = $("#model-settings").hidden;
    closePopup(); closeComposerMenus();
    if (!opening) return;
    modelQuery = "";
    $("#model-settings").hidden = false; $("#model-button").setAttribute("aria-expanded", "true");
    $("#model-settings").replaceChildren(element("p", "popover-empty muted", "正在读取原生模型…"));
    fitComposerMenu($("#model-settings"));
    try {
      await loadModels(true);
      if ($("#model-settings") && !$("#model-settings").hidden) {
        renderModelMenu(); $('#model-settings [aria-checked="true"]')?.focus();
      }
    } catch (error) {
      if ($("#model-settings") && !$("#model-settings").hidden) {
        $("#model-settings").replaceChildren(element("p", "popover-empty error-text", error.message));
        fitComposerMenu($("#model-settings"));
      }
    }
  };
  $("#model-settings").onkeydown = event => {
    if (event.isComposing) return;
    if (event.target === $("#model-search") && ["Home", "End"].includes(event.key)) return;
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const choices = [...$("#model-settings").querySelectorAll("button")];
    if (!choices.length) return;
    event.preventDefault();
    const current = choices.indexOf(document.activeElement);
    const index = event.key === "Home" ? 0 : event.key === "End" ? choices.length - 1
      : (current + (event.key === "ArrowDown" ? 1 : choices.length - 1)) % choices.length;
    choices[index].focus();
  };
  void loadModels().catch(() => {});
}
function startNewChat(projectId = $("#project-select").value) {
  if (!session || session.readOnly || !connected) return;
  closePopup(); rememberDraft();
  setSelectValue($("#project-select"), projectId);
  ++selectionVersion; creating = true; selectedId = null; thread = null; historyCursor = null; historyInitialized = false; turns.clear(); renderMessages();
  selectionToken = randomId();
  creationProject = $("#project-select").value; notifiedTokenUsage = null; ++threadReadVersion;
  restoreDraft(draftKey()); resetComposerOptions(); renderApprovals();
  $("#thread-title").textContent = "新建会话"; $("#thread-path").textContent = "选择本机已有项目";
  $("#project-select-slot").hidden = false;
  renderSidebar(); updateComposer(); $(".workspace").classList.remove("sidebar-open");
  $("#prompt").focus();
}
async function loadSidebar(cursor = null) {
  const version = ++sidebarVersion, deviceId = session?.device.id;
  const result = await api("/api/read/sidebar", { cursor });
  if (version !== sidebarVersion || !session || deviceId !== session.device.id) return;
  sidebar = result;
  renderSidebar();
}
function renderSidebar() {
  const root = $("#sidebar-content");
  if (!root || !sidebar) return;
  const scrollTop = root.scrollTop;
  const popupState = activePopup && root.contains(activePopup.trigger)
    ? { id: activePopup.trigger.id, index: activePopup.index, focused: activePopup.node.contains(document.activeElement), position: activePopup.position } : null;
  const focusId = root.contains(document.activeElement) ? document.activeElement.id : null;
  const focusedThread = document.activeElement.closest(".thread-row")?.dataset.threadId;
  const focusedMenu = document.activeElement.classList.contains("row-menu-button");
  if (popupState) closePopup();
  root.replaceChildren();
  setSelectValue($("#grouping"), sidebar.preferences.mode);
  setSelectValue($("#sorting"), sidebar.preferences.mode === "project" ? sidebar.preferences.projects : sidebar.preferences.chats);
  const disabled = session.readOnly || sidebarBusy || !connected;
  $("#grouping").disabled = $("#sorting").disabled = disabled;
  $("#sidebar-controls").setAttribute("aria-busy", String(sidebarBusy));
  const domId = (type, id) => `${type}-${encodeURIComponent(id)}`;
  const matches = item => !search || `${item.title || ""} ${item.name || ""} ${item.cwd || ""}`.toLowerCase().includes(search);
  const pinnedIds = new Set(sidebar.pinnedThreads.map(item => item.id));
  function row(item, scope = "recent") {
    const container = element("div", `thread-row${selectedId === item.id ? " selected" : ""}`);
    container.dataset.threadId = item.id;
    const title = button(item.title, item.title, () => void selectThread(item.id).catch(error => notice(error.message, true)), "thread-link");
    title.id = domId("thread", `${scope}:${item.id}`);
    title.replaceChildren(element("span", "thread-label", item.title));
    if (selectedId === item.id) title.setAttribute("aria-current", "page");
    const active = item.status?.type === "active" || item.status?.type === "inProgress";
    if (scope === "pinned") title.prepend(icon("chat"));
    container.append(title);
    if (active || item.hasUnreadTurn) {
      const state = element("span", `thread-state${active ? " active" : " unread"}`);
      state.title = active ? "正在运行" : "有未读消息"; state.setAttribute("aria-label", state.title);
      container.append(state);
    }
    if (!session.readOnly) {
      const pinned = pinnedIds.has(item.id) || item.pinned;
      const menu = rowMenu(domId("thread-menu", `${scope}:${item.id}`), `${item.title} 的会话菜单`, [
        { label: "打开会话", icon: "chat", action: () => void selectThread(item.id).catch(error => notice(error.message, true)) },
        { label: pinned ? "取消置顶会话" : "置顶会话", icon: "pin", action: () => changeSidebar({ threadId: item.id, pinned: !pinned }) }
      ]);
      container.append(menu);
      container.oncontextmenu = event => {
        event.preventDefault();
        openPopup(menu, { position: event.clientX || event.clientY ? { x: event.clientX, y: event.clientY } : null });
      };
    }
    return container;
  }
  const pinnedThreads = sidebar.pinnedThreads.filter(matches);
  if (pinnedThreads.length) {
    const heading = element("div", "section-label", "置顶");
    const sorting = selectControl("pinned-sorting", "置顶排序",
      [{ value: "manual", label: "手动" }, { value: "updated_at", label: "最近更新" }], sidebar.preferences.pinned, "pin-sort");
    sorting.disabled = disabled;
    sorting.onchange = () => void changeSidebar({ target: "pinned", mode: sorting.value });
    heading.append(sorting); root.append(heading);
    pinnedThreads.forEach(item => root.append(row(item, "pinned")));
  }
  if (sidebar.preferences.mode === "project") {
    root.append(element("div", "section-label", "项目"));
    for (const project of sidebar.projects) {
      const available = project.threads.filter(item => !pinnedIds.has(item.id));
      const items = matches(project) && search ? available : available.filter(matches);
      if (search && !matches(project) && !items.length) continue;
      const group = element("div", "project-group");
      const header = element("div", "project-header");
      const projectTitle = button("", project.name, () => {
        collapsed.has(project.id) ? collapsed.delete(project.id) : collapsed.add(project.id); renderSidebar();
      }, "project-title");
      projectTitle.id = domId("project", project.id);
      projectTitle.setAttribute("aria-expanded", String(!collapsed.has(project.id)));
      projectTitle.append(icon("folder"), element("span", "project-label", project.name));
      projectTitle.title = project.cwd || project.name;
      header.append(projectTitle);
      if (!session.readOnly) header.append(rowMenu(domId("project-menu", project.id), `${project.name} 的项目菜单`, [
        { label: "在此项目新建会话", icon: "plus", action: () => startNewChat(project.id) },
        { label: project.pinned ? "取消置顶项目" : "置顶项目", icon: "pin", action: () => changeSidebar({ projectId: project.id, pinned: !project.pinned }) }
      ]));
      group.append(header);
      if (!collapsed.has(project.id)) {
        const visible = search || expandedProjects.has(project.id) ? items : items.slice(0, 8);
        visible.forEach(item => group.append(row(item, project.id)));
        if (visible.length < items.length) group.append(button(`查看其余 ${items.length - visible.length} 个会话`, "显示更多项目会话", () => {
          expandedProjects.add(project.id); renderSidebar();
        }, "load-more"));
        if (!items.length) group.append(element("small", "empty-project", project.threads.length ? "会话已移至置顶区" : "暂无会话"));
      }
      root.append(group);
    }
    const unassigned = sidebar.recent.filter(item => !item.projectId && !pinnedIds.has(item.id) && matches(item));
    if (unassigned.length) { root.append(element("div", "section-label", "最近")); unassigned.forEach(item => root.append(row(item))); }
  } else {
    root.append(element("div", "section-label", "最近会话"));
    const recent = sidebar.recent.filter(item => !pinnedIds.has(item.id) && matches(item));
    const visible = search ? recent : recent.slice(0, recentLimit);
    visible.forEach(item => root.append(row(item)));
    if (visible.length < recent.length) root.append(button("显示更多会话", "显示更多会话", () => { recentLimit += 40; renderSidebar(); }, "load-more"));
  }
  if (sidebar.nextCursor) root.append(button("加载更多会话", "加载更多会话", () => {
    void loadSidebar(sidebar.nextCursor).catch(error => notice(error.message, true));
  }, "load-more"));
  if (search && !root.querySelector(".thread-row, .project-group")) root.append(element("p", "empty-project", "没有匹配的会话或项目"));
  setSelectOptions($("#project-select"), sidebar.projects.map(project => ({ value: project.id, label: project.name, description: project.cwd })));
  $("#project-select").disabled = session.readOnly || !sidebar.projects.length;
  changeCreationProject();
  root.scrollTop = scrollTop;
  if (popupState) {
    const trigger = document.getElementById(popupState.id);
    if (trigger && !trigger.disabled) {
      openPopup(trigger, { focus: false, position: popupState.position });
      if (activePopup) {
        activePopup.index = popupState.index;
        if (popupState.focused) activePopup.list.querySelectorAll("button")[popupState.index]?.focus({ preventScroll: true });
      }
    }
  } else if (focusId) {
    const restored = document.getElementById(focusId) || [...root.querySelectorAll(".thread-row")]
      .find(item => item.dataset.threadId === focusedThread)?.querySelector(focusedMenu ? ".row-menu-button" : ".thread-link");
    restored?.focus({ preventScroll: true });
  }
  fitPopup();
}
function inlineMarkdown(text) {
  const pieces = [];
  let value = escape(text).replace(/`([^`]+)`/g, (_, code) => {
    pieces.push(`<code>${code}</code>`); return `\u0000${pieces.length - 1}\u0000`;
  });
  value = value.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, raw) => {
    const url = raw.replace(/&amp;/g, "&");
    if (!/^https?:\/\//i.test(url) && !/^#[a-zA-Z0-9_-]+$/.test(url)) return label;
    return `<a href="${escape(url)}" target="_blank" rel="noopener noreferrer">${label}</a>`;
  }).replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>").replace(/(^|[^*])\*([^*]+)\*/g, "$1<em>$2</em>")
    .replace(/~~([^~]+)~~/g, "<del>$1</del>");
  return value.replace(/\u0000(\d+)\u0000/g, (_, index) => pieces[Number(index)] || "");
}
function markdown(text) {
  const lines = String(text || "").replaceAll("\r", "").split("\n");
  const html = [];
  let paragraph = [], list = null, code = null, language = "";
  const flush = () => { if (paragraph.length) { html.push(`<p>${paragraph.map(inlineMarkdown).join("<br>")}</p>`); paragraph = []; } };
  const closeList = () => { if (list) html.push(`</${list}>`); list = null; };
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const fence = line.match(/^\s*(`{3,}|~{3,})([\w+-]*)/);
    if (fence) {
      flush(); closeList();
      if (code == null) { code = []; language = fence[2]; }
      else {
        html.push(`<div class="code-block"><div class="code-header"><span>${escape(language || "代码")}</span><button class="copy-code" type="button">复制</button></div><pre><code>${escape(code.join("\n"))}</code></pre></div>`);
        code = null;
      }
      continue;
    }
    if (code) { code.push(line); continue; }
    if (line.includes("|") && /^\s*\|?\s*:?-{3,}/.test(lines[index + 1] || "")) {
      flush(); closeList();
      const cells = value => value.trim().replace(/^\||\|$/g, "").split("|").map(cell => inlineMarkdown(cell.trim()));
      const head = cells(line);
      html.push(`<div class="table-scroll"><table><thead><tr>${head.map(cell => `<th>${cell}</th>`).join("")}</tr></thead><tbody>`);
      index++;
      while (lines[index + 1]?.includes("|") && lines[index + 1].trim()) html.push(`<tr>${cells(lines[++index]).map(cell => `<td>${cell}</td>`).join("")}</tr>`);
      html.push("</tbody></table></div>"); continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.+)$/);
    const item = line.match(/^\s*(?:([-*+])|(\d+)[.)])\s+(.+)$/);
    if (heading) { flush(); closeList(); html.push(`<h${heading[1].length}>${inlineMarkdown(heading[2])}</h${heading[1].length}>`); }
    else if (item) {
      flush(); const kind = item[2] ? "ol" : "ul";
      if (kind !== list) { closeList(); html.push(`<${kind}>`); list = kind; }
      const check = item[3].match(/^\[([ xX])\]\s+(.*)$/);
      html.push(`<li>${check ? `${check[1].trim() ? "☑" : "☐"} ${inlineMarkdown(check[2])}` : inlineMarkdown(item[3])}</li>`);
    } else if (/^\s*>/.test(line)) { flush(); closeList(); html.push(`<blockquote>${inlineMarkdown(line.replace(/^\s*>\s?/, ""))}</blockquote>`); }
    else if (/^\s*(---+|\*\*\*+)\s*$/.test(line)) { flush(); closeList(); html.push("<hr>"); }
    else if (!line.trim()) { flush(); closeList(); }
    else { closeList(); paragraph.push(line); }
  }
  if (code) html.push(`<pre><code>${escape(code.join("\n"))}</code></pre>`);
  flush(); closeList();
  return html.join("");
}
const textContent = item => item.text || (Array.isArray(item.content) ? item.content.filter(content => content.type === "text").map(content => content.text).join("\n") : "");
function renderItem(item) {
  if (item.type === "userMessage") {
    const node = element("article", "message user-message");
    node.append(element("div", "user-bubble", textContent(item)));
    return node;
  }
  if (item.type === "agentMessage") {
    const node = element("article", `message agent-message${item.phase === "commentary" ? " commentary" : ""}`);
    const body = element("div", "markdown");
    body.innerHTML = markdown(textContent(item));
    node.append(body);
    for (const copy of node.querySelectorAll(".copy-code")) copy.onclick = () => copyText(copy.closest(".code-block").querySelector("code").textContent);
    return node;
  }
  const titles = {
    reasoning: "思考", commandExecution: "执行命令", fileChange: "修改文件", mcpToolCall: "调用工具",
    dynamicToolCall: "调用工具", webSearch: "搜索网页", plan: "工作计划", imageView: "查看图片", error: "执行出错"
  };
  const details = element("details", `tool-item${item.status === "failed" ? " failed" : ""}`);
  const summary = element("summary");
  const name = item.type === "mcpToolCall" ? `${item.server || ""} · ${item.tool || ""}`
    : item.type === "commandExecution" ? item.command || "" : item.type === "webSearch" ? item.action?.query || "" : "";
  summary.append(element("span", "tool-state", ["inProgress", "running"].includes(item.status) ? "◌" : item.status === "failed" ? "!" : "✓"),
    element("strong", "", titles[item.type] || item.type), element("span", "tool-description", name),
    element("small", "muted", item.status === "inProgress" ? "进行中" : item.exitCode != null ? `退出码 ${item.exitCode}` : ""));
  details.append(summary);
  const content = item.type === "commandExecution" ? `${item.command || ""}\n${item.aggregatedOutput || item.output || ""}`
    : item.type === "reasoning" ? [item.summary, item.content].flatMap(value => Array.isArray(value) ? value.map(entry => entry.text || entry) : [value || ""]).join("\n")
    : item.type === "fileChange" ? (item.changes || []).map(change => `${change.path || ""}\n${change.diff || ""}`).join("\n\n")
    : item.type === "error" ? item.message : JSON.stringify(item, null, 2);
  details.append(element("pre", "tool-output", typeof content === "string" ? content : JSON.stringify(content)));
  return details;
}
function renderMessages(keepScroll = true) {
  const root = $("#messages");
  if (!root) return;
  const bottom = root.scrollHeight - root.scrollTop - root.clientHeight < 100;
  const oldHeight = root.scrollHeight, oldTop = root.scrollTop;
  const openTools = new Set([...root.querySelectorAll("details[open]")].map(node => node.dataset.key));
  root.replaceChildren();
  if (historyCursor) {
    const load = button(historyLoading ? "加载中…" : "加载更早的消息", "加载更早的消息", () => void loadHistory(true), "load-more");
    load.disabled = historyLoading && historyLoadingVersion === selectionVersion;
    root.append(load);
  }
  const sorted = [...turns.values()];
  for (const turn of sorted) {
    const group = element("section", "turn");
    group.dataset.turnId = turn.id;
    for (const item of turn.items || []) {
      const node = renderItem(item);
      node.dataset.key = `${turn.id}:${item.id}`;
      if (node.tagName === "DETAILS" && openTools.has(node.dataset.key)) node.open = true;
      group.append(node);
    }
    if (turn.error) group.append(element("div", "turn-error", turn.error.message || "此轮执行失败"));
    if (turn.status === "interrupted") group.append(element("small", "turn-end muted", "已停止"));
    root.append(group);
  }
  if (!sorted.length) root.append(element("div", "empty-conversation", creating ? "选择项目并发送第一条消息。" : "此会话暂无消息。"));
  if (Array.isArray(thread?.queuedMessages) && thread.queuedMessages.length) {
    const queue = element("section", "queued-messages");
    queue.append(element("small", "muted", `原生消息队列 · ${thread.queuedMessages.length} 条待执行`));
    for (const message of thread.queuedMessages) queue.append(element("p", "", message.prompt || message.message?.prompt || message.text || "待执行消息"));
    root.append(queue);
  }
  if (keepScroll && bottom) root.scrollTop = root.scrollHeight;
  else if (!keepScroll) root.scrollTop = oldTop + root.scrollHeight - oldHeight;
}
async function selectThread(id, { closeSidebar = true } = {}) {
  if (closeSidebar) closePopup();
  rememberDraft();
  const version = ++selectionVersion;
  selectionToken = randomId();
  selectedId = id; creating = false; thread = null; turns.clear(); historyCursor = null; historyInitialized = false;
  restoreDraft(id); resetComposerOptions();
  notifiedTokenUsage = null;
  const read = ++threadReadVersion, usageVersion = tokenUsageVersion;
  renderApprovals();
  $("#project-select-slot").hidden = true;
  $("#messages").replaceChildren(element("p", "empty-conversation", "正在读取原生会话…"));
  if (closeSidebar) $(".workspace").classList.remove("sidebar-open");
  renderSidebar(); updateComposer();
  const result = await api("/api/read/thread", { threadId: id });
  if (version !== selectionVersion || read !== threadReadVersion || selectedId !== id) return;
  thread = result.thread;
  if (usageVersion !== tokenUsageVersion) thread.tokenUsage = notifiedTokenUsage;
  $("#thread-title").textContent = thread.title;
  $("#thread-path").textContent = thread.cwd;
  updateComposer(); renderApprovals(); await loadHistory(false, version);
  if (version !== selectionVersion) return;
  $("#messages").scrollTop = $("#messages").scrollHeight;
  syncThreadHash(id);
}
function syncThreadHash(id) {
  const hash = `#thread=${encodeURIComponent(id)}`;
  if (location.hash !== hash) history.pushState(null, "", hash);
}
function welcomeNode() {
  const node = element("div", "welcome");
  node.append(element("h1", "", "继续本机的工作"), element("p", "", "选择左侧会话，或在已有项目中新建会话。"), element("small", "", "消息、审批和执行状态与 Codex 桌面共享。"));
  return node;
}
function showWelcome() {
  rememberDraft();
  ++selectionVersion; ++threadReadVersion; selectionToken = randomId();
  selectedId = null; thread = null; creating = false; historyCursor = null; historyInitialized = false; turns.clear(); notifiedTokenUsage = null;
  $("#prompt").value = ""; selectedSkills = []; resetComposerOptions();
  $("#thread-title").textContent = "远程会话"; $("#thread-path").textContent = "";
  $("#project-select-slot").hidden = true;
  $("#messages").replaceChildren(welcomeNode());
  renderSelectedSkills(); renderApprovals(); renderSidebar(); updateComposer();
}
async function loadHistory(older = false, version = selectionVersion) {
  if (!selectedId || (historyLoading && historyLoadingVersion === version)) return;
  historyLoading = true; historyLoadingVersion = version;
  const id = selectedId;
  const initialIds = new Set(turns.keys());
  try {
    const result = await api("/api/read/history", { threadId: id, cursor: older ? historyCursor : null });
    if (version !== selectionVersion || selectedId !== id) return;
    const chronological = [...result.data].reverse();
    if (older) turns = new Map([...chronological.map(turn => [turn.id, turn]), ...turns]);
    else {
      const pageIds = new Set(chronological.map(turn => turn.id)), known = [...turns.values()];
      const firstShared = known.findIndex(turn => pageIds.has(turn.id));
      const earlier = historyInitialized ? known.filter((turn, index) => !pageIds.has(turn.id)
        && (firstShared < 0 ? initialIds.has(turn.id) : index < firstShared)) : [];
      const earlierIds = new Set(earlier.map(turn => turn.id));
      const later = known.filter(turn => !pageIds.has(turn.id) && !earlierIds.has(turn.id));
      turns = new Map([...earlier, ...chronological, ...later].map(turn => [turn.id, turn]));
    }
    if (older || !historyInitialized) historyCursor = result.nextCursor;
    historyInitialized = true;
    renderMessages(!older);
  } catch (error) { notice(error.message, true); }
  finally {
    if (historyLoadingVersion === version) {
      historyLoading = false;
      const load = $("#messages > .load-more");
      if (load) { load.disabled = false; load.textContent = "加载更早的消息"; }
    }
  }
}
async function reloadThread() {
  if (!selectedId) return;
  const id = selectedId, version = selectionVersion, read = ++threadReadVersion, usageVersion = tokenUsageVersion;
  const { thread: latest } = await api("/api/read/thread", { threadId: id });
  if (version !== selectionVersion || read !== threadReadVersion || selectedId !== id) return;
  if (usageVersion !== tokenUsageVersion) latest.tokenUsage = notifiedTokenUsage;
  thread = latest; $("#thread-title").textContent = thread.title;
  $("#thread-path").textContent = thread.cwd || ""; syncThreadHash(id);
  updateComposer(); renderApprovals(); await loadHistory(false, version);
}
function renderApprovals() {
  const root = $("#approval-area"); if (!root) return;
  const signature = JSON.stringify([thread?.id, thread?.requests || [], session?.readOnly, session?.allowApprovals]);
  if (signature === approvalSignature) return;
  approvalSignature = signature;
  root.replaceChildren();
  for (const request of thread?.requests || []) {
    const requestThreadId = thread.id;
    const card = element("section", "approval-card");
    card.append(element("strong", "", request.method === "item/tool/requestUserInput" ? "Codex 需要你回答" : "等待你的授权"));
    const network = request.params?.networkApprovalContext;
    const command = network ? `网络访问：${network.protocol || ""}://${network.host || ""}${network.port != null ? `:${network.port}` : ""}\n${request.params?.reason || ""}`
      : request.params?.command || request.params?.reason || request.params?.message || request.method;
    card.append(element("pre", "approval-description", typeof command === "string" ? command : JSON.stringify(command, null, 2)));
    if (session.readOnly || !session.allowApprovals) { card.append(element("small", "muted", "请在 Codex 桌面处理，此设备没有审批权限。")); root.append(card); continue; }
    const reply = async (action, value) => {
      for (const control of card.querySelectorAll("button,input,select,textarea")) control.disabled = true;
      try {
        await mutate(action, { threadId: requestThreadId, requestId: request.id, method: request.method, ...value });
        if (selectedId === requestThreadId) await reloadThread();
      }
      catch (error) { notice(error.message, true); for (const control of card.querySelectorAll("button,input,select,textarea")) control.disabled = false; }
    };
    const actions = element("div", "approval-actions");
    if (["item/commandExecution/requestApproval", "item/fileChange/requestApproval"].includes(request.method)) {
      const allowed = request.params?.availableDecisions;
      for (const [label, decision] of [["允许一次", "accept"], ["本会话允许", "acceptForSession"], ["拒绝", "decline"], ["取消本轮", "cancel"]]) {
        if (Array.isArray(allowed) && !allowed.includes(decision)) continue;
        actions.append(button(label, label, async () => {
          const deviceId = session?.device.id;
          if (decision === "acceptForSession" && !await confirmAction("本会话允许", "允许此会话后续同类操作，不再逐次询问。请确认已了解该操作范围。", "允许本会话")) return;
          if (!card.isConnected || session?.device.id !== deviceId || session.readOnly || !session.allowApprovals) return;
          void reply("approve", { decision });
        }, decision === "accept" ? "primary" : ""));
      }
      if (!actions.children.length) card.append(element("small", "muted", "此请求的审批方式请在 Codex 桌面处理。"));
    } else if (request.method === "item/tool/requestUserInput") {
      const fields = [];
      for (const question of request.params?.questions || []) {
        const label = element("label", "question-label", question.question || question.header);
        const input = element("input"); input.setAttribute("aria-label", question.question || question.header);
        if (question.isSecret) input.type = "password";
        const choices = element("div", "question-options");
        for (const option of question.options || []) choices.append(button(option.label, option.description || option.label, () => { input.value = option.label; }));
        fields.push([question.id, input]); label.append(choices, input); card.append(label);
      }
      actions.append(button("提交回答", "提交所有问题的回答", () => reply("answer", {
        response: { answers: Object.fromEntries(fields.map(([id, input]) => [id, { answers: [input.value] }])) }
      }), "primary"));
    } else if (request.method === "item/permissions/requestApproval") {
      card.append(element("pre", "approval-description", JSON.stringify(request.params?.permissions, null, 2)));
      actions.append(button("仅本轮授予", "仅本轮授予所列权限", () => reply("answer", { response: { grant: true, scope: "turn" } }), "primary"),
        button("拒绝", "不授予权限", () => reply("answer", { response: { grant: false, scope: "turn" } })));
    } else if (request.method === "mcpServer/elicitation/request" && ["form", "openai/form"].includes(request.params?.mode)) {
      const input = element("textarea"); input.placeholder = "按请求的 JSON 格式填写回答"; input.setAttribute("aria-label", "工具询问回答");
      card.append(input);
      actions.append(button("提交", "提交工具询问", () => {
        try { void reply("answer", { response: { action: "accept", content: JSON.parse(input.value) } }); }
        catch { notice("请输入有效 JSON", true); }
      }, "primary"), button("拒绝", "拒绝工具询问", () => reply("answer", { response: { action: "decline" } })));
    } else card.append(element("small", "muted", "此类请求请在 Codex 桌面处理。"));
    card.append(actions); root.append(card);
  }
}
function updateComposer() {
  if (!$("#composer")) return;
  const status = thread?.status?.status || thread?.status;
  const active = status?.type === "active" || status?.type === "inProgress";
  $("#stop").hidden = !active || session.readOnly || !selectedId;
  $("#stop").disabled = !connected;
  $("#send").disabled = busy || !!pending || !connected || session.readOnly || !composerPrompt() || (!creating && (!selectedId || thread?.id !== selectedId));
  $("#prompt").disabled = session.readOnly || (!creating && !selectedId);
  $("#new-chat").disabled = session.readOnly || !connected;
  $("#model-button").disabled = session.readOnly;
  $("#grouping").disabled = $("#sorting").disabled = session.readOnly || sidebarBusy || !connected;
  $("#project-select").disabled = session.readOnly || !sidebar?.projects.length;
  for (const control of document.querySelectorAll(".row-menu-button,#pinned-sorting")) control.disabled = session.readOnly || sidebarBusy || !connected;
  fitPopup();
  $("#prompt").placeholder = session.readOnly ? "当前设备为只读模式" : active ? "补充要求，会使用原生消息队列…" : "给 Codex 发送消息，输入 / 选择 Skill…";
  const model = currentModel(), effort = selectedEffort || thread?.reasoningEffort;
  const name = model?.displayName || selectedModel || thread?.model || "继承原生模型";
  $("#model-button").replaceChildren(element("span", "model-name", name));
  if (effort) $("#model-button").append(document.createTextNode(" "), element("span", "model-effort", effortLabels[effort] || effort));
  $("#model-button").title = `模型：${name}${effort ? ` · 思考深度：${effortLabels[effort] || effort}` : ""}`;
  const usage = contextUsage(thread?.tokenUsage), context = $("#context-info");
  context.replaceChildren();
  if (usage) {
    context.append(element("span", "", `已用${tokenCount(usage.used)} /`), element("span", "context-percent", `${usage.percent}%`), element("span", "", `，共${tokenCount(usage.total)}`));
    context.title = `最后一次请求的上下文占用：${usage.used.toLocaleString()} / ${usage.total.toLocaleString()} Token，不是累计用量`;
  } else { context.textContent = "上下文信息尚未提供"; context.removeAttribute("title"); }
  if (session.readOnly) closeComposerMenus();
  refitComposerMenus();
  $("#pending-area").replaceChildren();
  if (pending) {
    const unavailable = pending.invalid || (pending.deviceId && pending.deviceId !== session.device.id);
    const box = element("div", "pending-banner", unavailable ? "回执损坏或属于旧授权。请检查原生会话后人工解除锁定，不会自动重发。" : "消息正在确认送达，已锁定重复发送。");
    if (!unavailable) box.append(button("查回执", "查询此消息的送达回执", () => void resolvePending(), "text-button"));
    box.append(button("已人工确认", "确认检查会话后解除锁定", async () => {
      const current = pending;
      if (await confirmAction("解除发送锁定", "请先检查本机或网页会话。解除锁定不会重发，但再次发送可能重复执行；仅在已核对结果后继续。", "已核对，解除锁定", true)
          && pending === current) { savePending(null); updateComposer(); }
    }, "text-button"));
    $("#pending-area").append(box);
  }
}
async function sendMessage() {
  if (busy || pending || session.readOnly || !connected) return;
  const prompt = composerPrompt();
  if (!prompt) { notice("请输入消息"); return; }
  if (!selectedId && !creating) return;
  const id = selectedId;
  const version = selectionVersion;
  const wasCreating = creating;
  const payload = {
    prompt, ...(creating ? { projectId: $("#project-select").value } : { threadId: id }),
    ...(selectedModel ? { model: selectedModel } : {}),
    ...(selectedEffort ? { reasoningEffort: selectedEffort } : {}),
    ...(selectedSkills.length ? { skills: selectedSkills.map(({ name, path }) => ({ name, path })) } : {})
  };
  if (creating && !payload.projectId) { notice("请选择项目", true); return; }
  busy = true; updateComposer();
  try {
    const result = await mutate(wasCreating ? "create" : "send", payload, { reliable: true });
    clearSubmittedDraft(payload, selectionVersion === version);
    if (selectionVersion === version) {
      if (result.threadId && result.threadId !== selectedId) {
        if (wasCreating) await selectCreatedThread(result.threadId);
        else await selectThread(result.threadId);
      }
      else await reloadThread();
    } else notice(wasCreating ? "会话已在原项目创建，当前项目未被切换。" : "消息已发送到原会话，当前会话未被切换。");
    await loadSidebar();
  } catch (error) { notice(error.message, true); }
  finally { busy = false; updateComposer(); }
}
function setConnection(value) {
  connected = value;
  const node = $("#connection-state"); if (!node) return;
  node.textContent = value ? "本机已连接" : "连接中断 · 自动重连";
  node.classList.toggle("online", value);
  updateComposer();
}
function scheduleReload() {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => void reloadThread().catch(error => notice(error.message, true)), 450);
}
function receive(event) {
  if (event.method === "remote/unauthorized") { disconnect(); showLogin(); return; }
  if (event.method === "remote/disconnected") { setConnection(false); return; }
  if (["remote/reconnected", "remote/resync"].includes(event.method)) {
    setConnection(true); void refreshAll().catch(error => notice(error.message, true)); return;
  }
  const params = event.params || {};
  if (params.threadId === selectedId) {
    if (event.method === "thread/tokenUsage/updated") {
      notifiedTokenUsage = params.tokenUsage; ++tokenUsageVersion;
      if (thread) { thread.tokenUsage = params.tokenUsage; updateComposer(); }
    }
    const turnId = params.turnId || params.turn?.id;
    if (turnId) {
      let turn = turns.get(turnId);
      if (!turn) { turn = { id: turnId, status: "inProgress", items: [] }; turns.set(turnId, turn); }
      if (event.method === "turn/started") turn.status = "inProgress";
      if (event.method === "turn/completed") { Object.assign(turn, params.turn); scheduleReload(); }
      if (params.item) {
        const index = turn.items.findIndex(item => item.id === params.item.id);
        if (index < 0) turn.items.push(params.item); else turn.items[index] = params.item;
      }
      if (params.itemId && params.delta != null) {
        let item = turn.items.find(item => item.id === params.itemId);
        if (!item) {
          item = { id: params.itemId, type: event.method.includes("commandExecution") ? "commandExecution" : event.method.includes("reasoning") ? "reasoning" : "agentMessage", text: "", status: "inProgress" };
          turn.items.push(item);
        }
        if (event.method.includes("commandExecution")) item.aggregatedOutput = (item.aggregatedOutput || "") + params.delta;
        else if (event.method.includes("reasoning")) item.summary = (typeof item.summary === "string" ? item.summary : "") + params.delta;
        else item.text = (item.text || "") + params.delta;
      }
      renderMessages();
    }
    if (["thread/status/changed", "thread/settings/updated"].includes(event.method)) scheduleReload();
  }
  if (event.method === "skills/changed") { skillScope = ""; skillChoices = []; skillLoading = null; ++skillLoadVersion; if ($("#skill-menu") && !$("#skill-menu").hidden) void updateSkillMenu(); }
  if (event.method === "remote/requestsChanged" || event.method === "serverRequest/resolved") scheduleReload();
  if (["thread/started", "turn/completed", "remote/sidebarChanged", "thread/name/updated", "thread/status/changed"].includes(event.method)) {
    clearTimeout(sidebarTimer); sidebarTimer = setTimeout(() => void loadSidebar().catch(() => {}), 1200);
  }
}
function disconnect() {
  stream?.close(); stream = null; clearInterval(connect.poll);
  clearTimeout(refreshTimer); clearTimeout(sidebarTimer); clearTimeout(pendingTimer);
  clearTimeout(streamRetry); streamRetry = null;
  ++selectionVersion; ++sidebarVersion; connected = false;
}
async function refreshAll() {
  await loadSidebar();
  if (selectedId) await reloadThread();
  else if (!creating) {
    const id = new URLSearchParams(location.hash.slice(1)).get("thread");
    if (id) await selectThread(id, { closeSidebar: false });
  }
}
function openStream() {
  stream?.close();
  const current = new EventSource("/api/events");
  stream = current;
  const deviceId = session.device.id;
  const isCurrent = () => stream === current && session?.device.id === deviceId;
  current.onopen = async () => {
    streamBackoff = 1000;
    try {
      const latest = await api("/api/session");
      if (!isCurrent()) return;
      session = latest; setConnection(latest.nativeConnected);
      await refreshAll(); void resolvePending();
    } catch (error) { if (isCurrent()) notice(error.message, true); }
  };
  current.onmessage = event => { if (isCurrent()) { try { receive(JSON.parse(event.data)); } catch {} } };
  current.onerror = () => {
    if (!isCurrent()) return;
    setConnection(false);
    if (current.readyState === EventSource.CLOSED) reconnectStream(streamBackoff = Math.min(streamBackoff * 2, 30000));
  };
}
function reconnectStream(delay = streamBackoff) {
  clearTimeout(streamRetry);
  streamRetry = setTimeout(async () => {
    streamRetry = null;
    try { session = await api("/api/session"); openStream(); }
    catch { if (session) reconnectStream(streamBackoff = Math.min(streamBackoff * 2, 30000)); }
  }, delay);
}
function ensureStream() {
  if (session && !streamRetry && (!stream || stream.readyState === EventSource.CLOSED)) reconnectStream(0);
}
async function connect() {
  session = await api("/api/session");
  shell(); setConnection(session.nativeConnected);
  openStream();
  const deviceId = session.device.id;
  clearInterval(connect.poll);
  connect.poll = setInterval(async () => {
    if (document.hidden || session?.device.id !== deviceId) return;
    ensureStream();
    try {
      const current = await api("/api/session");
      if (session?.device.id !== deviceId) return;
      session = current; setConnection(current.nativeConnected && stream?.readyState === EventSource.OPEN); renderApprovals();
      await loadSidebar();
    } catch {}
  }, 15000);
  try { await refreshAll(); }
  catch (error) { if (session?.device.id === deviceId) notice(error.message, true); }
  updateComposer(); void resolvePending();
}
async function copyText(text) {
  try { await navigator.clipboard.writeText(text); notice("已复制"); }
  catch { notice("无法访问剪贴板，请手动选择代码复制", true); }
}
window.addEventListener("online", () => { if (session) { ensureStream(); void refreshAll().catch(() => {}); } });
document.addEventListener("visibilitychange", () => { if (!document.hidden && session) { ensureStream(); void refreshAll().catch(() => {}); } });
window.addEventListener("popstate", () => {
  if (!session) return;
  const id = new URLSearchParams(location.hash.slice(1)).get("thread");
  if (id && id !== selectedId) void selectThread(id, { closeSidebar: false }).catch(error => notice(error.message, true));
  else if (!id && selectedId) showWelcome();
});
connect().catch(error => { showLogin(); if (error.code !== 401) notice(error.message, true); });
