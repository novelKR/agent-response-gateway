// Tokenize serialized JSON without interpreting or changing its values.
export function jsonTokens(text) {
  const pattern = /"(?:[^"\\]|\\[\s\S])*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}\[\],:]|\s+/g;
  const tokens = [];
  let offset = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > offset) tokens.push({ kind: 'plain', text: text.slice(offset, match.index) });
    const value = match[0];
    const end = match.index + value.length;
    const kind = value[0] === '"' ? (/^\s*:/.test(text.slice(end)) ? 'key' : 'string')
      : /^-?\d/.test(value) ? 'number'
      : value === 'null' ? 'null'
      : /^(true|false)$/.test(value) ? 'boolean'
      : /^\s/.test(value) ? 'plain' : 'punctuation';
    tokens.push({ kind, text: value });
    offset = end;
  }
  if (offset < text.length) tokens.push({ kind: 'plain', text: text.slice(offset) });
  return tokens;
}
