(() => {
  const root = typeof window === "object" ? window : {};
  const id = "[A-Za-z_$][\\w$]*";
  const chatgpt = "`chatgpt`";
  const token = "`personalAccessToken`";
  const bedrock = "`amazonBedrock`";
  const apiKey = "`apiKey`";
  const local = "`local`";
  const critical = "`critical`";
  let modelAliases = new Map();

  const runtime = {
    update(settings) {
      if (settings && Object.hasOwn(settings, "modelAliases")) {
        modelAliases = new Map(Object.entries(settings.modelAliases || {})
          .filter(([, model]) => typeof model === "string")
          .map(([alias, model]) => [alias.trim().toLowerCase(), model.trim().toLowerCase()]));
      }
      root.__codexElvesNativeFastSettings = settings && typeof settings === "object" ? settings : null;
      return runtime.status();
    },
    status() {
      const settings = root.__codexElvesNativeFastSettings;
      const enhancementsEnabled = settings?.enhancementsEnabled === true;
      const optedIn = settings?.codexAppNativeFastUnlock === true;
      return {
        enabled: enhancementsEnabled && optedIn,
        enhancementsEnabled,
        optedIn,
        settingsLoaded: !!settings && typeof settings === "object",
      };
    },
    isGptModel(model) {
      if (typeof model !== "string" || !model.trim()) return false;
      const name = model.trim().toLowerCase();
      const requested = modelAliases.get(name) ?? name;
      return /^gpt-\d/.test(requested.split("/").at(-1));
    },
    allow(hostId, authMethod) {
      return hostId === "local"
        && (authMethod === "apikey" || authMethod === "apiKey")
        && runtime.status().enabled;
    },
  };

  function allow(host, auth) {
    return `window.__codexElvesNativeFastUnlock?.runtime?.allow?.(${host},${auth})===true`;
  }

  function gpt(model) {
    return `window.__codexElvesNativeFastUnlock?.runtime?.isGptModel?.(${model})===true`;
  }

  const queueDefaultPrefix = [
    `function ${id}\\((?<manager>${id}),\\{runtime:(?<runtime>${id}),storage:(?<storage>${id})\\},(?<before>${id}),${id}\\)\\{let (?<pending>${id})=null;`,
    `return\\{async readServiceTier\\((?<model>${id})\\)\\{for\\(;\\k<pending>!=null;\\)await \\k<pending>\\.catch\\(\\(\\)=>void 0\\);`,
    `return \\k<before>\\(\\),${id}\\(\\k<manager>\\.requestClient,\\k<manager>\\.logger,\\k<model>,async\\(\\)=>`,
  ].join("");
  const queueCopilotCondition = `\\k<manager>\\.getHostId\\(\\)===${local}&&await \\k<storage>\\.readGlobalState\\(\`use-copilot-auth-if-available\`\\)===!0&&await \\k<runtime>\\.isCopilotApiAvailable\\(\\)`;

  const queueDefaultPrefixModern = [
    `function ${id}\\((?<manager>${id}),\\{runtime:(?<runtime>${id})\\},(?<before>${id}),${id}\\)\\{let (?<pending>${id})=null;`,
    `return\\{async readServiceTier\\((?<model>${id})\\)\\{for\\(;\\k<pending>!=null;\\)await \\k<pending>\\.catch\\(\\(\\)=>void 0\\);`,
    `return \\k<before>\\(\\),${id}\\(\\k<manager>\\.requestClient,\\k<manager>\\.logger,\\k<model>,async\\(\\)=>`,
  ].join("");
  const queueCopilotConditionModern = `\\k<manager>\\.getHostId\\(\\)===${local}&&await \\k<runtime>\\.isUsingCopilotInference\\(\\)`;

  // 同时核对四条身份路径及当前模型的 UI、请求调用点；不依赖压缩变量名。
  // 原生菜单也会只传 hostId 读取账号能力；有 model 字段时才校验模型，
  // ui-model 始终显式传入当前模型，因此空值和非 GPT 仍无法产生 Fast 选项。
  // 不修改 requirements、模型 serviceTiers 或 Standard 的解析规则。
  const gates = [
    {
      kind: "ui-access",
      pattern: [
        `function ${id}\\((?<argument>${id})\\)\\{let ${id}=\\(0,${id}\\.c\\)\\(\\d+\\),(?<fallback>${id})=${id}\\(${id}\\),`,
        `(?<host>${id})=\\k<argument>\\?\\.hostId\\?\\?\\k<fallback>,(?<identity>${id})=${id}\\(\\k<host>\\),`,
        `(?<variable>${id})=(?<gate>\\k<identity>\\?\\.authMethod===${chatgpt}\\|\\|\\k<identity>\\?\\.authMethod===${token}(?:\\|\\|\\k<identity>\\?\\.authMethod===${bedrock})?),`,
        `(?<method>${id})=\\k<identity>\\?\\.authMethod\\?\\?null,[^{}]{0,300}\\{authMethod:\\k<method>,hostId:\\k<host>\\}`,
        `[^{}]{0,300}\\{data:(?<data>${id}),isPending:(?<pending>${id})\\}=${id}\\(${id},${id}\\),`,
        `(?<loading>${id})=!!\\k<identity>\\?\\.isLoading\\|\\|\\k<variable>&&\\k<pending>,${id};`,
        `[^{}]{0,300}\\k<variable>&&!\\k<loading>&&\\k<data>!=null\\?${id}\\(\\k<data>\\):null`,
        `[^{}]{0,300}\\{serviceTierAccess:${id},isLoading:\\k<loading>\\}`,
      ].join(""),
      replacement: (groups) => `((!Object.hasOwn(${groups.argument}??{},'model')||${gpt(`${groups.argument}?.model`)})&&(${groups.gate}||${allow(groups.host, `${groups.identity}?.authMethod`)}))`,
    },
    {
      kind: "request-access",
      pattern: [
        `async function ${id}\\((?<scope>${id}),(?<host>${id})\\)\\{let (?<auth>${id})=await ${id}\\(\\k<scope>,\\k<host>\\);`,
        `if\\((?<gate>\\k<auth>!==${chatgpt}&&\\k<auth>!==${token}(?:&&\\k<auth>!==${bedrock})?)\\)return null;`,
        `let (?<requirements>${id})=await ${id}\\(\\k<scope>,\\k<host>,\\{priority:${critical}\\}\\);`,
        `return \\k<scope>\\.query\\.setData\\(${id},\\{authMethod:\\k<auth>,hostId:\\k<host>\\},\\k<requirements>\\),${id}\\(\\k<requirements>\\)\\}`,
      ].join(""),
      replacement: (groups) => `${groups.gate}&&!(${allow(groups.host, groups.auth)})`,
    },
    {
      kind: "queue-default-access",
      patterns: [
        [
          queueDefaultPrefix,
          `${queueCopilotCondition}\\?!1:`,
          `(?<gate>(?<account>\\(await \\k<manager>\\.getAccount\\(\\{priority:${critical}\\}\\)\\)\\.account\\?\\.type)===${chatgpt})\\)\\},async writeServiceTier\\(`,
        ].join(""),
        [
          queueDefaultPrefix,
          `\\{if\\(${queueCopilotCondition}\\)return!1;`,
          `let (?<accountResult>${id})=await \\k<manager>\\.getAccount\\(\\{priority:${critical}\\}\\);`,
          `return (?<gate>(?<account>\\k<accountResult>\\.account\\?\\.type)===${chatgpt}(?<bedrock>\\|\\|\\k<accountResult>\\.account\\?\\.type===${bedrock})?)\\}\\)\\},async writeServiceTier\\(`,
        ].join(""),
        [
          queueDefaultPrefixModern,
          `\\{if\\(${queueCopilotConditionModern}\\)return!1;`,
          `let (?<accountResult>${id})=await \\k<manager>\\.getAccount\\(\\{priority:${critical}\\}\\);`,
          `return (?<gate>(?<account>\\k<accountResult>\\.account\\?\\.type)===${chatgpt}(?<bedrock>\\|\\|\\k<accountResult>\\.account\\?\\.type===${bedrock})?)\\}\\)\\},async writeServiceTier\\(`,
        ].join(""),
      ],
      // includes 只消费一次 account/read；保留各版本原有账号和 Copilot 分支。
      replacement: (groups) => `(${gpt(groups.model)}&&[${chatgpt}${groups.bedrock ? `,${bedrock}` : ""},...(${allow(`${groups.manager}.getHostId()`, apiKey)}?[${apiKey}]:[])].includes(${groups.account}))`,
    },
    {
      kind: "queue-explicit-access",
      patterns: [
        [
          `function ${id}\\((?<manager>${id}),\\{settings:(?<settings>${id}),storage:(?<storage>${id}),filesystem:${id},runtime:(?<runtime>${id})\\},(?<readDefault>${id}),${id},${id}\\)\\{`,
          `return async\\(${id},(?<message>${id})\\)=>\\{let (?<options>${id})=\\k<message>\\.submissionOptions,${id}=\\k<message>\\.context\\.workPermissions,${id}=${id}!=null,`,
          `(?<host>${id})=\\k<options>\\?\\.executionHostId\\?\\?\\k<manager>\\.getHostId\\(\\);`,
          `[\\s\\S]{0,400}?let (?<conversation>${id})=\\k<manager>\\.getConversation\\(${id}\\),${id}=${id}\\([^;]{0,200}\\),(?<collaboration>${id})=\\k<options>\\?\\.collaborationMode\\?\\?null;`,
          `[\\s\\S]{0,1800}?let\\[${id},${id}\\]=await Promise\\.all\\(\\[\\(async\\(\\)=>\\{if\\(\\k<options>==null\\)return \\k<readDefault>\\(${id}\\?\\.latestModel\\|\\|null\\);`,
          `if\\(\\k<options>\\.serviceTier==null\\)return null;let\\{account:(?<account>${id})\\}=await \\k<manager>\\.getAccount\\(\\{priority:${critical}\\}\\);`,
          `return (?<gate>\\k<account>\\?\\.type===${chatgpt}(?:\\|\\|\\k<account>\\?\\.type===${bedrock})?)\\?\\k<options>\\.serviceTier:null\\}\\)\\(\\),\\k<options>\\?\\.usePermissionSelection\\?\\?\\k<settings>\\.readPermissionRefreshEnabled\\(\\)\\]\\)`,
        ].join(""),
        [
          `function ${id}\\((?<manager>${id}),\\{settings:(?<settings>${id}),storage:(?<storage>${id}),filesystem:${id},runtime:(?<runtime>${id})\\},(?<readDefault>${id}),${id},${id}\\)\\{`,
          `return async\\(${id},(?<message>${id})\\)=>\\{let (?<options>${id})=\\k<message>\\.submissionOptions,${id}=\\k<message>\\.context\\.workPermissions,${id}=${id}!=null,`,
          `(?<host>${id})=\\k<options>\\?\\.executionHostId\\?\\?\\k<manager>\\.getHostId\\(\\);`,
          `[\\s\\S]*?let (?<conversation>${id})=\\k<manager>\\.getConversation\\(${id}\\),`,
          `[\\s\\S]*?(?<collaboration>${id})=\\k<options>\\?\\.collaborationMode\\?\\?null;`,
          `[\\s\\S]*?let\\[${id},${id}\\]=await Promise\\.all\\(\\[\\(async\\(\\)=>\\{if\\(\\k<options>==null\\)return \\k<readDefault>\\(${id}\\?\\.latestModel\\|\\|null\\);if\\(\\k<options>\\.serviceTier==null\\)return null;let\\{account:(?<account>${id})\\}=await \\k<manager>\\.getAccount\\(\\{priority:${critical}\\}\\);return (?<gate>\\k<account>\\?\\.type===${chatgpt}(?:\\|\\|\\k<account>\\?\\.type===${bedrock})?)\\?\\k<options>\\.serviceTier:null\\}\\)\\(\\),`,
        ].join(""),
      ],
      // manager 和实际执行端都必须是本机；所选 Copilot 不能借底层 API 身份放行。
      replacement: (groups) => `(${gpt(`${groups.collaboration}?.settings?.model??${groups.conversation}?.latestModel`)}&&(${groups.gate}||${groups.host}===${local}&&${allow(`${groups.manager}.getHostId()`, `${groups.account}?.type`)}&&!(await ${groups.storage}.readGlobalState(\`use-copilot-auth-if-available\`)===!0&&await ${groups.runtime}.isCopilotApiAvailable())))`,
    },
    {
      kind: "ui-model",
      pattern: [
        `function ${id}\\(${id},(?<settings>${id}),${id},${id},${id}\\)\\{let ${id}=\\(0,${id}\\.c\\)\\(\\d+\\),`,
        `\\{modelSettings:(?<modelSettings>${id}),setModelAndReasoningEffort:${id}\\}=\\k<settings>`,
        `[\\s\\S]{0,1400}?let\\{serviceTierAccess:${id}\\}=(?<gate>(?<hook>${id})\\((?<options>${id})\\)),`,
      ].join(""),
      // 原生 React 模型设置是每个会话/草稿的当前选择，不能回落到全局默认模型。
      replacement: (groups) => `${groups.hook}({...${groups.options},model:${groups.modelSettings}.model})`,
    },
    {
      kind: "request-model",
      pattern: [
        `async function ${id}\\((?<scope>${id}),(?<host>${id}),(?<model>${id}),(?<override>${id})\\)\\{try\\{let (?<access>${id})=`,
        `(?<gate>await ${id}\\(\\k<scope>,\\k<host>\\));if\\(\\k<override>!==void 0\\)return ${id}\\(null,\\k<override>,\\k<access>\\);`,
      ].join(""),
      replacement: (groups) => `(${gpt(groups.model)}?${groups.gate}:null)`,
    },
  ];

  function plan(source, url) {
    const diagnostics = { url, reason: "ready", matches: {} };
    const failed = (reason) => ({ ok: false, replacements: [], diagnostics: { ...diagnostics, reason } });
    if (typeof source !== "string" || !source || typeof url !== "string" || !url.trim()) {
      return failed("invalid-input");
    }
    const replacements = [];
    for (const definition of gates) {
      const rawMatches = (definition.patterns ?? [definition.pattern])
        .flatMap((pattern) => [...source.matchAll(new RegExp(pattern, "g"))]);
      const matches = [];
      const locations = new Set();
      for (const match of rawMatches) {
        const gate = match.groups?.gate;
        const offset = typeof gate === "string" ? match[0].indexOf(gate) : -1;
        const key = offset >= 0
          ? `${match.index + offset}:${match.index + offset + gate.length}`
          : `${match.index}:${match[0]}`;
        if (locations.has(key)) continue;
        locations.add(key);
        matches.push(match);
      }
      diagnostics.matches[definition.kind] = matches.length;
      if (matches.length !== 1) continue;
      const match = matches[0];
      const gate = match.groups.gate;
      const offset = match[0].indexOf(gate);
      if (offset < 0 || offset !== match[0].lastIndexOf(gate) || /[\r\n]/.test(gate)) {
        return failed("unsupported-shape");
      }
      const start = match.index + offset;
      const text = definition.replacement(match.groups);
      if (/[\r\n]/.test(text)) return failed("unsupported-shape");
      replacements.push({ kind: definition.kind, start, end: start + gate.length, text });
    }
    if (Object.values(diagnostics.matches).some((count) => count > 1)) return failed("ambiguous-gates");
    if (replacements.length !== gates.length) return failed("incomplete-gates");
    replacements.sort((left, right) => left.start - right.start);
    if (replacements.some((replacement, index) => index > 0 && replacement.start < replacements[index - 1].end)) {
      return failed("overlapping-gates");
    }
    return { ok: true, replacements, diagnostics };
  }

  function transform(source, url) {
    const result = plan(source, url);
    let patchedSource = source;
    if (result.ok) {
      for (const replacement of [...result.replacements].reverse()) {
        patchedSource = patchedSource.slice(0, replacement.start) + replacement.text + patchedSource.slice(replacement.end);
      }
    }
    return { ...result, patchedSource };
  }

  function moduleUrl(value) {
    if (typeof value !== "string" || /[\r\n]/.test(value)) return null;
    try {
      const parsed = new URL(value);
      return (parsed.protocol === "app:" && parsed.hostname === "-"
        || parsed.protocol === "file:" && !parsed.hostname) ? parsed : null;
    } catch { return null; }
  }

  // 只分词，不执行源码。模板的原始文本、注释和正则都不参与模块路径识别。
  // 正则字面量边界采用 js-tokens 4 的词法规则，并补充现代 d/v 标志。
  /*
   * Copyright (c) 2014, 2015, 2016, 2017, 2018 Simon Lydell
   * Permission is hereby granted, free of charge, to any person obtaining a copy
   * of this software and associated documentation files (the "Software"), to deal
   * in the Software without restriction, including without limitation the rights
   * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
   * copies of the Software, and to permit persons to whom the Software is
   * furnished to do so, subject to the following conditions:
   * The above copyright notice and this permission notice shall be included in
   * all copies or substantial portions of the Software.
   * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
   * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
   * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
   * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
   * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
   * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
   * SOFTWARE.
   */
  function moduleTokens(source) {
    const regex = /\/(?:\[(?:[^\]\\\r\n]|\\[^\r\n])*\]|[^\/\\\r\n[]|\\[^\r\n])+\/[dgimsuvy]*/y;
    const identifier = /[$A-Z_a-z\u0080-\uFFFF][$\w\u0080-\uFFFF]*/y;
    const number = /(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\d[\d_]*(?:\.[\d_]*)?|\.\d[\d_]*)(?:[eE][+-]?[\d_]+)?)n?/y;
    const operator = /(?:\?\.|\+\+|--|=>|\*\*|&&|\|\||\?\?|===|!==|==|!=|<=|>=|<<|>>>|>>|\.{3}|[+\-*%&|^!?<>=]=?)/y;
    let position = 0;
    let expressionAllowed = true;
    let last = null;
    const parentheses = [];
    const blocks = [];
    const emit = (token) => {
      const value = token.value;
      if (token.type === "name") expressionAllowed = /^(?:return|throw|case|delete|void|typeof|new|in|instanceof|yield|await|else|do)$/.test(value);
      else if (["string", "regex", "number", "template-end"].includes(token.type)) expressionAllowed = false;
      else if (value === "(") { parentheses.push(/^(?:if|while|for|with|switch|catch)$/.test(last?.value)); expressionAllowed = true; }
      else if (value === ")") expressionAllowed = parentheses.pop() === true;
      else if (value === "{") {
        blocks.push(!["=", "(", "[", ",", ":", "?", "return", "yield"].includes(last?.value));
        expressionAllowed = true;
      } else if (value === "}") expressionAllowed = blocks.pop() !== false;
      else expressionAllowed = ![".", "?.", "]", "++", "--"].includes(value);
      last = token;
      return token;
    };
    function* code(templateExpression = false) {
      let braces = 0;
      while (position < source.length) {
        const start = position;
        const char = source[position];
        if (/\s/.test(char)) { position++; continue; }
        if (source.startsWith("//", position)) {
          const end = source.indexOf("\n", position + 2);
          position = end < 0 ? source.length : end + 1;
          continue;
        }
        if (source.startsWith("/*", position)) {
          const end = source.indexOf("*/", position + 2);
          if (end < 0) throw new Error("unterminated-comment");
          position = end + 2;
          continue;
        }
        if (char === "'" || char === '"') {
          position++;
          for (; position < source.length; position++) {
            if (source[position] === "\\") { position++; continue; }
            if (source[position] === char) break;
            if (/[\r\n]/.test(source[position])) throw new Error("unterminated-string");
          }
          if (position >= source.length) throw new Error("unterminated-string");
          yield emit({ type: "string", start, end: ++position, value: source.slice(start, position) });
          continue;
        }
        if (char === "`") {
          position++;
          let interpolated = false;
          for (;;) {
            if (position >= source.length) throw new Error("unterminated-template");
            if (source[position] === "\\") { position += 2; continue; }
            if (source[position] === "`") {
              position++;
              yield emit({ type: interpolated ? "template-end" : "string", start, end: position, value: source.slice(start, position) });
              break;
            }
            if (source.startsWith("${", position)) {
              if (!interpolated) yield emit({ type: "template-start", start, end: position, value: "`" });
              interpolated = true;
              position += 2;
              expressionAllowed = true;
              yield* code(true);
              continue;
            }
            position++;
          }
          continue;
        }
        if (char === "/" && expressionAllowed) {
          regex.lastIndex = position;
          const match = regex.exec(source);
          if (match) {
            position = regex.lastIndex;
            yield emit({ type: "regex", start, end: position, value: match[0] });
            continue;
          }
        }
        identifier.lastIndex = position;
        const name = identifier.exec(source);
        if (name) {
          position = identifier.lastIndex;
          yield emit({ type: "name", start, end: position, value: name[0] });
          continue;
        }
        number.lastIndex = position;
        const numeric = number.exec(source);
        if (numeric) {
          position = number.lastIndex;
          yield emit({ type: "number", start, end: position, value: numeric[0] });
          continue;
        }
        operator.lastIndex = position;
        const operation = operator.exec(source);
        if (operation) {
          position = operator.lastIndex;
          yield emit({ type: "punctuator", start, end: position, value: operation[0] });
          continue;
        }
        position++;
        if (char === "{") braces++;
        if (char === "}") {
          if (templateExpression && braces === 0) return;
          braces--;
        }
        yield emit({ type: "punctuator", start, end: position, value: char });
      }
      if (templateExpression) throw new Error("unterminated-template-expression");
    }
    return code();
  }

  function stringValue(token) {
    // 模块 specifier 一律静态解析，禁止 eval / Function；不认识的转义直接拒绝。
    const text = token.value.slice(1, -1);
    let value = "";
    for (let index = 0; index < text.length; index++) {
      const char = text[index];
      if (char !== "\\") { value += char; continue; }
      const escaped = text[++index];
      if (escaped == null) throw new Error("invalid-module-string");
      if (escaped === "\r" || escaped === "\n") {
        if (escaped === "\r" && text[index + 1] === "\n") index++;
        continue;
      }
      const simple = { n: "\n", r: "\r", t: "\t", b: "\b", f: "\f", v: "\v", "0": "\0" };
      if (Object.hasOwn(simple, escaped)) { value += simple[escaped]; continue; }
      if (escaped === "x" || escaped === "u") {
        const rest = text.slice(index + 1);
        const match = escaped === "x" ? /^[\da-fA-F]{2}/.exec(rest)
          : /^(?:[\da-fA-F]{4}|\{[\da-fA-F]+\})/.exec(rest);
        if (!match) throw new Error("invalid-module-string");
        const hex = match[0].replace(/[{}]/g, "");
        value += String.fromCodePoint(parseInt(hex, 16));
        index += match[0].length;
      } else if (/[1-9]/.test(escaped)) throw new Error("invalid-module-string");
      else value += escaped;
    }
    return value;
  }

  function rewriteModuleBase(source, url) {
    const iterator = moduleTokens(source);
    const buffered = [];
    const edits = [];
    const dependencies = [];
    const counts = { staticImports: 0, dynamicImports: 0, importMetaUrls: 0 };
    let previous = null;
    const peek = (offset = 0) => {
      while (buffered.length <= offset) {
        const next = iterator.next();
        buffered.push(next.done ? null : next.value);
      }
      return buffered[offset];
    };
    const take = () => { const token = peek(); buffered.shift(); if (token) previous = token; return token; };
    const replaceString = (token, kind) => {
      const specifier = stringValue(token);
      counts[kind]++;
      if (specifier.startsWith(".") || specifier.startsWith("/")) {
        const resolved = new URL(specifier, url).href;
        dependencies.push(resolved);
        edits.push({ start: token.start, end: token.end, text: JSON.stringify(resolved) });
      }
    };
    for (;;) {
      const before = previous;
      const token = take();
      if (!token) break;
      if (token.type !== "name" || [".", "?."].includes(before?.value)) continue;
      if (token.value === "import") {
        if ([":", ",", "}"].includes(peek()?.value)) continue;
        if (peek()?.value === ".") {
          if (peek(1)?.value !== "meta" || peek(2)?.value !== "." || peek(3)?.value !== "url") {
            throw new Error("unsupported-import-meta");
          }
          take(); take(); take();
          const end = take().end;
          edits.push({ start: token.start, end, text: JSON.stringify(url) });
          counts.importMetaUrls++;
        } else if (peek()?.value === "(") {
          // 对象和类的 import(...) 方法名不是 ImportExpression。
          if (peek(1)?.type !== "string") {
            let depth = 0;
            let offset = 0;
            do {
              const part = peek(offset++);
              if (!part) break;
              if (part.value === "(") depth++;
              if (part.value === ")") depth--;
            } while (depth > 0);
            if (depth === 0 && peek(offset)?.value === "{") continue;
          }
          take();
          const argument = take();
          if (argument?.type !== "string" || ![",", ")"].includes(peek()?.value)) {
            throw new Error("nonliteral-dynamic-import");
          }
          replaceString(argument, "dynamicImports");
        } else {
          let specifier = take();
          if (specifier?.type !== "string") {
            while (specifier && specifier.value !== "from" && specifier.value !== ";") specifier = take();
            specifier = specifier?.value === "from" ? take() : null;
          }
          if (specifier?.type !== "string") throw new Error("unsupported-static-import");
          replaceString(specifier, "staticImports");
        }
      } else if (token.value === "export" && ["*", "{"].includes(peek()?.value)) {
        if (peek().value === "{") {
          while (peek() && take().value !== "}") { /* 只消费 export specifier 列表。 */ }
          if (peek()?.value !== "from") continue;
        } else {
          while (peek() && peek().value !== "from" && peek().value !== ";") take();
        }
        if (take()?.value !== "from") throw new Error("unsupported-static-export");
        const specifier = take();
        if (specifier?.type !== "string") throw new Error("unsupported-static-export");
        replaceString(specifier, "staticImports");
      }
    }
    const chunks = [];
    let offset = 0;
    for (const edit of edits) {
      if (edit.start < offset) throw new Error("overlapping-module-edits");
      chunks.push(source.slice(offset, edit.start), edit.text);
      offset = edit.end;
    }
    chunks.push(source.slice(offset));
    return { source: chunks.join(""), counts, dependencies };
  }

  function moduleDependencies(source, url) {
    return rewriteModuleBase(source, url).dependencies;
  }

  function prepareModule(source, url) {
    const failed = (reason, diagnostics = {}) => ({ ok: false, patchedSource: source, diagnostics: { url, ...diagnostics, reason } });
    if (!runtime.status().enabled) return failed("disabled");
    if (!moduleUrl(url)) return failed("unsupported-module-url");
    const transformed = transform(source, url);
    if (!transformed.ok) return transformed;
    try {
      const rewritten = rewriteModuleBase(transformed.patchedSource, url);
      return {
        ...transformed,
        patchedSource: `${rewritten.source}\n;window.__codexElvesNativeFastCompiled="v3";window.__codexElvesNativeFastPatchState="patched";window.__codexElvesNativeFastModuleComplete?.();\n//# sourceURL=${url}\n`,
        diagnostics: { ...transformed.diagnostics, ...rewritten.counts, reason: "module-prepared" },
      };
    } catch (error) {
      return failed(error instanceof Error ? error.message : "unsupported-module-syntax", transformed.diagnostics);
    }
  }

  function installModule(patchedSource, url) {
    const diagnostics = { url, reason: "module-preloaded" };
    const failed = (reason) => {
      diagnostics.reason = reason;
      root.__codexElvesNativeFastPatchState = "failed";
      root.__codexElvesNativeFastModuleDiagnostics = diagnostics;
      return { ok: false, diagnostics };
    };
    if (!runtime.status().enabled) return { ok: false, diagnostics: { url, reason: "disabled" } };
    const document = root.document;
    if (!moduleUrl(url) || !moduleUrl(root.location?.href)) return { ok: false, diagnostics: { url, reason: "unsupported-document-url" } };
    if (!document || document.documentElement) return failed("document-already-started");
    if (typeof patchedSource !== "string" || !patchedSource.includes('window.__codexElvesNativeFastCompiled="v3";')) return failed("unprepared-module");
    let temporaryRoot = null;
    try {
      // import map 不能撤销。预加载失败时仅重载一次，下一文档跳过映射并正常加载原模块。
      const fallbackKey = "codex-elves-native-fast-fallback-v3";
      const storage = root.sessionStorage;
      if (!storage) return failed("fallback-storage-unavailable");
      if (storage.getItem(fallbackKey) === url) {
        storage.removeItem(fallbackKey);
        return failed("module-preload-failed-open");
      }
      storage.setItem(fallbackKey, "checking");
      storage.removeItem(fallbackKey);
      if (root.HTMLScriptElement?.supports?.("importmap") !== true) return failed("importmap-unavailable");
      const map = document.createElement("script");
      const preload = document.createElement("link");
      if (preload.relList?.supports?.("modulepreload") !== true) return failed("modulepreload-unavailable");
      const blob = root.URL.createObjectURL(new root.Blob([patchedSource], { type: "text/javascript" }));
      map.type = "importmap";
      map.textContent = JSON.stringify({ imports: { [url]: blob } });
      preload.rel = "modulepreload";
      preload.href = blob;
      let watchdog = null;
      let recovering = false;
      const cleanup = () => {
        if (watchdog != null) { root.clearTimeout(watchdog); watchdog = null; }
        document.removeEventListener("DOMContentLoaded", armWatchdog);
        if (root.__codexElvesNativeFastModuleComplete === complete) delete root.__codexElvesNativeFastModuleComplete;
      };
      const recover = (reason) => {
        if (recovering || root.__codexElvesNativeFastCompiled === "v3") return;
        recovering = true;
        cleanup();
        failed(reason);
        try {
          storage.setItem(fallbackKey, url);
          root.location.reload();
        } catch { /* 存储失败时禁止重载，避免重载循环。 */ }
      };
      const complete = () => {
        if (root.__codexElvesNativeFastCompiled === "v3") cleanup();
      };
      const armWatchdog = () => {
        if (root.__codexElvesNativeFastCompiled === "v3") { cleanup(); return; }
        if (watchdog != null) return;
        // 不监听普通 error/unhandledrejection。只检查目标模块自己的执行完成标记。
        watchdog = root.setTimeout(() => {
          watchdog = null;
          recover("module-execution-timeout");
        }, 30000);
      };
      preload.onerror = () => recover("module-preload-failed");
      temporaryRoot = document.createElement("html");
      document.appendChild(temporaryRoot);
      temporaryRoot.appendChild(map);
      temporaryRoot.appendChild(preload);
      root.__codexElvesNativeFastModuleComplete = complete;
      document.addEventListener("DOMContentLoaded", armWatchdog, { once: true });
      if (document.readyState !== "loading") armWatchdog();
      root.__codexElvesNativeFastModuleDiagnostics = diagnostics;
      return { ok: true, diagnostics };
    } catch {
      return failed("module-install-failed");
    } finally {
      // 必须同步移除临时根；保留它会让原 HTML parser 的 head/body 脱离文档。
      if (temporaryRoot?.parentNode === document) document.removeChild(temporaryRoot);
    }
  }

  const api = { plan, transform, prepareModule, installModule, moduleDependencies, runtime };
  if (typeof module === "object" && module.exports) module.exports = api;
  if (typeof window === "object") window.__codexElvesNativeFastUnlock = api;
})();
