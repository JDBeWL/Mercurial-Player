/** 轻量 Markdown 渲染器, 面向 GitHub Release Notes; 文本一律先 HTML 转义再拼标签, 防 XSS */

/** 转义 HTML 特殊字符, 是所有行内渲染的前置步骤 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/**
 * 校验 URL 协议: 只放行 http/https/mailto 与无协议的相对路径, 其余返回 null。
 *
 * 目的是挡掉 javascript:/data: 之类可注入协议, 调用方拿到 null 时降级为纯文本。
 */
function sanitizeUrl(url: string): string | null {
  // 控制字符会拆坏前缀(如 jav[TAB]ascript:), 含控制字符直接拒绝
  // eslint-disable-next-line no-control-regex -- 安全检查需要匹配控制字符
  if (/[\u0000-\u001f\u007f]/.test(url)) return null

  const trimmed = url.trim().toLowerCase()
  const colonIndex = trimmed.indexOf(':')

  // 无冒号即相对路径或锚点, 视为安全
  if (colonIndex === -1) return url

  // 协议段还要形如合法 scheme, 避免把 "foo bar:" 之类当成协议
  const scheme = trimmed.slice(0, colonIndex)
  if (!/^[a-z][a-z0-9+.-]*$/.test(scheme)) return null
  if (scheme === 'http' || scheme === 'https' || scheme === 'mailto') return url

  return null
}

/** 行内语法渲染; 替换顺序即优先级, 不要重排 */
function renderInline(text: string): string {
  let result = escapeHtml(text)

  // 行内代码最先处理: 反引号内的内容不再做二次替换
  result = result.replace(/`([^`]+)`/g, '<code>$1</code>')

  // 图片必须先于链接匹配, 否则 ![..](..) 会被当成链接; 非法 URL 降级见 sanitizeUrl
  result = result.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_match, alt: string, url: string) => {
    const safeUrl = sanitizeUrl(url)
    return safeUrl === null
      ? alt
      : `<img src="${safeUrl}" alt="${alt}" style="max-width:100%;border-radius:4px;" />`
  })

  // 链接 [text](url); 非法 URL 只保留文本, 降级规则见 sanitizeUrl
  result = result.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, text: string, url: string) => {
    const safeUrl = sanitizeUrl(url)
    return safeUrl === null
      ? text
      : `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${text}</a>`
  })

  result = result.replace(/\*{3}(.+?)\*{3}/g, '<strong><em>$1</em></strong>')
  result = result.replace(/_{3}(.+?)_{3}/g, '<strong><em>$1</em></strong>')

  result = result.replace(/\*{2}(.+?)\*{2}/g, '<strong>$1</strong>')
  result = result.replace(/_{2}(.+?)_{2}/g, '<strong>$1</strong>')

  // 斜体靠前后断言排除成对的 ** 与 __, 否则粗体会被拆坏
  result = result.replace(/(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)/g, '<em>$1</em>')
  result = result.replace(/(?<!_)_(?!_)(.+?)(?<!_)_(?!_)/g, '<em>$1</em>')

  result = result.replace(/~~(.+?)~~/g, '<del>$1</del>')

  return result
}

function isUnorderedListItem(line: string): boolean {
  return /^[-*+]\s+/.test(line.trim())
}

function isOrderedListItem(line: string): boolean {
  return /^\d+\.\s+/.test(line.trim())
}

function getListItemContent(line: string): string {
  return line.trim().replace(/^[-*+]\s+|^\d+\.\s+/, '')
}

/** 表格分隔行: 剥掉首尾 | 后逐格判定, 每格只含 - : 空格且至少一个 -, 如 |---| 或 |:---:| */
function isTableSeparator(line: string): boolean {
  const trimmed = line.trim()
  if (!trimmed.includes('|')) return false
  const inner = trimmed.replace(/^\||\|$/g, '')
  const cells = inner.split('|')
  if (cells.length === 0) return false
  return cells.every((cell) => {
    const c = cell.trim()
    return c !== '' && /^[-:]+$/.test(c) && c.includes('-')
  })
}

/** 表格行 -> 单元格数组(已 trim), 分隔行与数据行共用 */
function parseTableRow(line: string): string[] {
  const trimmed = line.trim()
  const inner = trimmed.replace(/^\||\|$/g, '')
  return inner.split('|').map((cell) => cell.trim())
}

/** Markdown -> HTML: 先按行做块级切分, 块内文本再交给 renderInline */
export function renderMarkdown(markdown: string): string {
  if (!markdown || typeof markdown !== 'string') return ''

  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const htmlParts: string[] = []
  let i = 0

  while (i < lines.length) {
    const line = lines[i]!
    const trimmed = line.trim()

    if (trimmed === '') {
      i++
      continue
    }

    if (trimmed.startsWith('```')) {
      const codeLines: string[] = []
      i++ // 跳过开始的 ```
      while (i < lines.length && !lines[i]!.trim().startsWith('```')) {
        codeLines.push(escapeHtml(lines[i]!))
        i++
      }
      i++ // 跳过结束的 ```; 缺失结束标记时整块吃到文件末尾
      htmlParts.push(`<pre><code>${codeLines.join('\n')}</code></pre>`)
      continue
    }

    const headingMatch = trimmed.match(/^(#{1,6})\s+(.+)$/)
    if (headingMatch) {
      const level = headingMatch[1]!.length
      const content = renderInline(headingMatch[2]!)
      htmlParts.push(`<h${level}>${content}</h${level}>`)
      i++
      continue
    }

    // 水平线判定要先于列表, 否则 --- / *** 会被当作列表项吞掉
    if (/^[-*_]{3,}$/.test(trimmed)) {
      htmlParts.push('<hr />')
      i++
      continue
    }

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

    // GFM 表格: 仅当表头行的下一行是分隔行才认定为表格, 光有 | 不算
    if (trimmed.includes('|') && i + 1 < lines.length && isTableSeparator(lines[i + 1]!)) {
      const headerCells = parseTableRow(trimmed)
      i += 2 // 跳过表头和分隔行

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

    // 列表只支持单层, 缩进不产生嵌套
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

    // 兜底段落: 连续取行, 直到空行或遇到任一块级起始标记
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
      // 表格起始行的判定与上面分支保持一致, 否则表格会被段落吃掉
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
