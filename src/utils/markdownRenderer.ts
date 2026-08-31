/**
 * 轻量级 Markdown 渲染器
 *
 * 专为 GitHub Release Notes 设计，支持常用 Markdown 语法。
 * 输出经过 HTML 转义，防止 XSS 攻击。
 */

/** HTML 特殊字符转义 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/**
 * 校验 URL 协议：仅允许 http/https/mailto（或无协议的相对路径）
 * 防止 javascript:、data: 等危险协议注入；不合法时返回 null
 */
function sanitizeUrl(url: string): string | null {
  // 拒绝包含控制字符的 URL（如 "jav\tascript:" 可绕过简单前缀检查）
  // eslint-disable-next-line no-control-regex -- 安全检查需要匹配控制字符
  if (/[\u0000-\u001f\u007f]/.test(url)) return null

  const trimmed = url.trim().toLowerCase()
  const colonIndex = trimmed.indexOf(':')

  // 无协议：相对路径或锚点，视为安全
  if (colonIndex === -1) return url

  // 协议部分必须是合法的 scheme，且在白名单内
  const scheme = trimmed.slice(0, colonIndex)
  if (!/^[a-z][a-z0-9+.-]*$/.test(scheme)) return null
  if (scheme === 'http' || scheme === 'https' || scheme === 'mailto') return url

  return null
}

/** 处理行内 Markdown 语法（bold、italic、code、link 等） */
function renderInline(text: string): string {
  let result = escapeHtml(text)

  // 行内代码 `code`（最先处理，内部不再解析其他语法）
  result = result.replace(/`([^`]+)`/g, '<code>$1</code>')

  // 图片 ![alt](url) — 在链接之前匹配；URL 协议不合法时仅保留 alt 文本
  result = result.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_match, alt: string, url: string) => {
    const safeUrl = sanitizeUrl(url)
    return safeUrl === null
      ? alt
      : `<img src="${safeUrl}" alt="${alt}" style="max-width:100%;border-radius:4px;" />`
  })

  // 链接 [text](url)；URL 协议不合法时仅保留链接文本
  result = result.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, text: string, url: string) => {
    const safeUrl = sanitizeUrl(url)
    return safeUrl === null
      ? text
      : `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${text}</a>`
  })

  // 粗斜体 ***text*** 或 ___text___
  result = result.replace(/\*{3}(.+?)\*{3}/g, '<strong><em>$1</em></strong>')
  result = result.replace(/_{3}(.+?)_{3}/g, '<strong><em>$1</em></strong>')

  // 粗体 **text** 或 __text__
  result = result.replace(/\*{2}(.+?)\*{2}/g, '<strong>$1</strong>')
  result = result.replace(/_{2}(.+?)_{2}/g, '<strong>$1</strong>')

  // 斜体 *text* 或 _text_（排除 ** 和 __）
  result = result.replace(/(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)/g, '<em>$1</em>')
  result = result.replace(/(?<!_)_(?!_)(.+?)(?<!_)_(?!_)/g, '<em>$1</em>')

  // 删除线 ~~text~~
  result = result.replace(/~~(.+?)~~/g, '<del>$1</del>')

  return result
}

/** 判断是否是无序列表项 */
function isUnorderedListItem(line: string): boolean {
  return /^[-*+]\s+/.test(line.trim())
}

/** 判断是否是有序列表项 */
function isOrderedListItem(line: string): boolean {
  return /^\d+\.\s+/.test(line.trim())
}

/** 提取列表项内容 */
function getListItemContent(line: string): string {
  return line.trim().replace(/^[-*+]\s+|^\d+\.\s+/, '')
}

/** 判断是否为表格分隔行（如 |---|---| 或 |:---:|---:|） */
function isTableSeparator(line: string): boolean {
  const trimmed = line.trim()
  if (!trimmed.includes('|')) return false
  // 去掉首尾的 |，按 | 分割后，每个单元格应只包含 -、:、空格，且至少有一个 -
  const inner = trimmed.replace(/^\||\|$/g, '')
  const cells = inner.split('|')
  if (cells.length === 0) return false
  return cells.every((cell) => {
    const c = cell.trim()
    return c !== '' && /^[-:]+$/.test(c) && c.includes('-')
  })
}

/** 解析表格行，返回各单元格内容（已 trim） */
function parseTableRow(line: string): string[] {
  const trimmed = line.trim()
  // 去掉首尾的 |，然后按 | 分割
  const inner = trimmed.replace(/^\||\|$/g, '')
  return inner.split('|').map((cell) => cell.trim())
}

/**
 * 将 Markdown 文本渲染为 HTML
 */
export function renderMarkdown(markdown: string): string {
  if (!markdown || typeof markdown !== 'string') return ''

  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const htmlParts: string[] = []
  let i = 0

  while (i < lines.length) {
    const line = lines[i]!
    const trimmed = line.trim()

    // —— 空行 ——
    if (trimmed === '') {
      i++
      continue
    }

    // —— 代码块 ```  ——
    if (trimmed.startsWith('```')) {
      const codeLines: string[] = []
      i++ // 跳过开始的 ```
      while (i < lines.length && !lines[i]!.trim().startsWith('```')) {
        codeLines.push(escapeHtml(lines[i]!))
        i++
      }
      i++ // 跳过结束的 ```
      htmlParts.push(`<pre><code>${codeLines.join('\n')}</code></pre>`)
      continue
    }

    // —— 标题 # ——
    const headingMatch = trimmed.match(/^(#{1,6})\s+(.+)$/)
    if (headingMatch) {
      const level = headingMatch[1]!.length
      const content = renderInline(headingMatch[2]!)
      htmlParts.push(`<h${level}>${content}</h${level}>`)
      i++
      continue
    }

    // —— 水平线 ---, ***, ___ ——
    if (/^[-*_]{3,}$/.test(trimmed)) {
      htmlParts.push('<hr />')
      i++
      continue
    }

    // —— 引用块 > ——
    if (trimmed.startsWith('>')) {
      const quoteLines: string[] = []
      while (i < lines.length && lines[i]!.trim().startsWith('>')) {
        quoteLines.push(lines[i]!.trim().replace(/^>\s?/, ''))
        i++
      }
      const quoteContent = renderMarkdown(quoteLines.join('\n'))
      htmlParts.push(`<blockquote>${quoteContent}</blockquote>`)
      continue
    }

    // —— 表格（GFM 风格：| header | ... | 后跟 |---|---| 分隔行）——
    if (trimmed.includes('|') && i + 1 < lines.length && isTableSeparator(lines[i + 1]!)) {
      const headerCells = parseTableRow(trimmed)
      i += 2 // 跳过表头和分隔行

      // 收集数据行
      const bodyRows: string[][] = []
      while (i < lines.length && lines[i]!.trim().includes('|') && lines[i]!.trim() !== '') {
        bodyRows.push(parseTableRow(lines[i]!))
        i++
      }

      const headerHtml = headerCells.map((cell) => `<th>${renderInline(cell)}</th>`).join('')
      const bodyHtml = bodyRows
        .map((row) => `<tr>${row.map((cell) => `<td>${renderInline(cell)}</td>`).join('')}</tr>`)
        .join('')
      htmlParts.push(
        `<table><thead><tr>${headerHtml}</tr></thead><tbody>${bodyHtml}</tbody></table>`,
      )
      continue
    }

    // —— 无序列表 ——
    if (isUnorderedListItem(trimmed)) {
      const items: string[] = []
      while (i < lines.length && isUnorderedListItem(lines[i]!.trim())) {
        items.push(renderInline(getListItemContent(lines[i]!)))
        i++
      }
      const itemsHtml = items.map((item) => `<li>${item}</li>`).join('')
      htmlParts.push(`<ul>${itemsHtml}</ul>`)
      continue
    }

    // —— 有序列表 ——
    if (isOrderedListItem(trimmed)) {
      const items: string[] = []
      while (i < lines.length && isOrderedListItem(lines[i]!.trim())) {
        items.push(renderInline(getListItemContent(lines[i]!)))
        i++
      }
      const itemsHtml = items.map((item) => `<li>${item}</li>`).join('')
      htmlParts.push(`<ol>${itemsHtml}</ol>`)
      continue
    }

    // —— 普通段落 ——
    const paragraphLines: string[] = []
    while (
      i < lines.length &&
      lines[i]!.trim() !== '' &&
      !lines[i]!.trim().startsWith('#') &&
      !lines[i]!.trim().startsWith('```') &&
      !lines[i]!.trim().startsWith('>') &&
      !/^[-*_]{3,}$/.test(lines[i]!.trim()) &&
      !isUnorderedListItem(lines[i]!.trim()) &&
      !isOrderedListItem(lines[i]!.trim()) &&
      // 表格起始行：当前行含 | 且下一行是分隔行
      !(lines[i]!.trim().includes('|') && i + 1 < lines.length && isTableSeparator(lines[i + 1]!))
    ) {
      paragraphLines.push(lines[i]!.trim())
      i++
    }
    if (paragraphLines.length > 0) {
      const content = paragraphLines.map((l) => renderInline(l)).join('<br />')
      htmlParts.push(`<p>${content}</p>`)
    }
  }

  return htmlParts.join('')
}
