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

/** 处理行内 Markdown 语法（bold、italic、code、link 等） */
function renderInline(text: string): string {
  let result = escapeHtml(text)

  // 行内代码 `code`（最先处理，内部不再解析其他语法）
  result = result.replace(/`([^`]+)`/g, '<code>$1</code>')

  // 图片 ![alt](url) — 在链接之前匹配
  result = result.replace(
    /!\[([^\]]*)\]\(([^)]+)\)/g,
    '<img src="$2" alt="$1" style="max-width:100%;border-radius:4px;" />'
  )

  // 链接 [text](url)
  result = result.replace(
    /\[([^\]]+)\]\(([^)]+)\)/g,
    '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>'
  )

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

/**
 * 将 Markdown 文本渲染为 HTML
 */
export function renderMarkdown(markdown: string): string {
  if (!markdown || typeof markdown !== 'string') return ''

  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const htmlParts: string[] = []
  let i = 0

  while (i < lines.length) {
    const line = lines[i]
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
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        codeLines.push(escapeHtml(lines[i]))
        i++
      }
      i++ // 跳过结束的 ```
      htmlParts.push(`<pre><code>${codeLines.join('\n')}</code></pre>`)
      continue
    }

    // —— 标题 # ——
    const headingMatch = trimmed.match(/^(#{1,6})\s+(.+)$/)
    if (headingMatch) {
      const level = headingMatch[1].length
      const content = renderInline(headingMatch[2])
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
      while (i < lines.length && lines[i].trim().startsWith('>')) {
        quoteLines.push(lines[i].trim().replace(/^>\s?/, ''))
        i++
      }
      const quoteContent = renderMarkdown(quoteLines.join('\n'))
      htmlParts.push(`<blockquote>${quoteContent}</blockquote>`)
      continue
    }

    // —— 无序列表 ——
    if (isUnorderedListItem(trimmed)) {
      const items: string[] = []
      while (i < lines.length && isUnorderedListItem(lines[i].trim())) {
        items.push(renderInline(getListItemContent(lines[i])))
        i++
      }
      const itemsHtml = items.map(item => `<li>${item}</li>`).join('')
      htmlParts.push(`<ul>${itemsHtml}</ul>`)
      continue
    }

    // —— 有序列表 ——
    if (isOrderedListItem(trimmed)) {
      const items: string[] = []
      while (i < lines.length && isOrderedListItem(lines[i].trim())) {
        items.push(renderInline(getListItemContent(lines[i])))
        i++
      }
      const itemsHtml = items.map(item => `<li>${item}</li>`).join('')
      htmlParts.push(`<ol>${itemsHtml}</ol>`)
      continue
    }

    // —— 普通段落 ——
    const paragraphLines: string[] = []
    while (
      i < lines.length &&
      lines[i].trim() !== '' &&
      !lines[i].trim().startsWith('#') &&
      !lines[i].trim().startsWith('```') &&
      !lines[i].trim().startsWith('>') &&
      !/^[-*_]{3,}$/.test(lines[i].trim()) &&
      !isUnorderedListItem(lines[i].trim()) &&
      !isOrderedListItem(lines[i].trim())
    ) {
      paragraphLines.push(lines[i].trim())
      i++
    }
    if (paragraphLines.length > 0) {
      const content = paragraphLines.map(l => renderInline(l)).join('<br />')
      htmlParts.push(`<p>${content}</p>`)
    }
  }

  return htmlParts.join('')
}

