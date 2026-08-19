import { describe, it, expect } from 'vitest'
import { renderMarkdown } from '@/utils/markdownRenderer'

describe('renderMarkdown', () => {
  it('should return empty string for empty input', () => {
    expect(renderMarkdown('')).toBe('')
    expect(renderMarkdown(null as unknown as string)).toBe('')
    expect(renderMarkdown(undefined as unknown as string)).toBe('')
  })

  it('should escape HTML characters', () => {
    const result = renderMarkdown('<script>alert("xss")</script>')
    expect(result).toContain('&lt;script&gt;')
    expect(result).not.toContain('<script>')
  })

  describe('headings', () => {
    it('should render h1', () => {
      expect(renderMarkdown('# Title')).toBe('<h1>Title</h1>')
    })

    it('should render h2', () => {
      expect(renderMarkdown('## Subtitle')).toBe('<h2>Subtitle</h2>')
    })

    it('should render h6', () => {
      expect(renderMarkdown('###### Small')).toBe('<h6>Small</h6>')
    })

    it('should render heading with inline formatting', () => {
      expect(renderMarkdown('# **Bold** Title')).toBe('<h1><strong>Bold</strong> Title</h1>')
    })
  })

  describe('code blocks', () => {
    it('should render fenced code block', () => {
      const md = '```\nconst x = 1\n```'
      expect(renderMarkdown(md)).toBe('<pre><code>const x = 1</code></pre>')
    })

    it('should escape code content', () => {
      const md = '```\n<div>hello</div>\n```'
      expect(renderMarkdown(md)).toBe('<pre><code>&lt;div&gt;hello&lt;/div&gt;</code></pre>')
    })
  })

  describe('inline code', () => {
    it('should render inline code', () => {
      expect(renderMarkdown('use `npm install`')).toBe('<p>use <code>npm install</code></p>')
    })
  })

  describe('bold and italic', () => {
    it('should render bold with **', () => {
      expect(renderMarkdown('**bold**')).toBe('<p><strong>bold</strong></p>')
    })

    it('should render italic with *', () => {
      expect(renderMarkdown('*italic*')).toBe('<p><em>italic</em></p>')
    })

    it('should render bold italic with ***', () => {
      expect(renderMarkdown('***both***')).toBe('<p><strong><em>both</em></strong></p>')
    })

    it('should render strikethrough', () => {
      expect(renderMarkdown('~~deleted~~')).toBe('<p><del>deleted</del></p>')
    })
  })

  describe('links and images', () => {
    it('should render links', () => {
      expect(renderMarkdown('[link](https://example.com)')).toBe(
        '<p><a href="https://example.com" target="_blank" rel="noopener noreferrer">link</a></p>',
      )
    })

    it('should render images', () => {
      expect(renderMarkdown('![alt](image.png)')).toBe(
        '<p><img src="image.png" alt="alt" style="max-width:100%;border-radius:4px;" /></p>',
      )
    })

    it('should render http links', () => {
      expect(renderMarkdown('[link](http://example.com)')).toContain('href="http://example.com"')
    })

    it('should render mailto links', () => {
      expect(renderMarkdown('[mail](mailto:a@b.com)')).toContain('href="mailto:a@b.com"')
    })

    it('should strip javascript: protocol links', () => {
      const result = renderMarkdown('[click](javascript:location=//evil.com)')
      expect(result).not.toContain('javascript:')
      expect(result).not.toContain('<a ')
      expect(result).toContain('click')
    })

    it('should strip data: protocol links', () => {
      const result = renderMarkdown('[x](data:text/html,<script>)')
      expect(result).not.toContain('data:')
      expect(result).not.toContain('<a ')
    })

    it('should strip javascript: protocol from control-character obfuscated links', () => {
      const result = renderMarkdown('[click](jav\tascript:alert(1))')
      expect(result).not.toContain('alert(1)')
      expect(result).not.toContain('<a ')
    })

    it('should strip javascript: protocol images', () => {
      const result = renderMarkdown('![img](javascript:alert(1))')
      expect(result).not.toContain('javascript:')
      expect(result).not.toContain('<img')
    })

    it('should render relative links without protocol', () => {
      expect(renderMarkdown('[rel](docs/page.html)')).toContain('href="docs/page.html"')
      expect(renderMarkdown('[anchor](#section)')).toContain('href="#section"')
    })
  })

  describe('lists', () => {
    it('should render unordered list', () => {
      const md = '- item 1\n- item 2'
      expect(renderMarkdown(md)).toBe('<ul><li>item 1</li><li>item 2</li></ul>')
    })

    it('should render ordered list', () => {
      const md = '1. first\n2. second'
      expect(renderMarkdown(md)).toBe('<ol><li>first</li><li>second</li></ol>')
    })
  })

  describe('quotes', () => {
    it('should render blockquote', () => {
      const md = '> quote'
      expect(renderMarkdown(md)).toBe('<blockquote><p>quote</p></blockquote>')
    })

    it('should render nested blockquote', () => {
      const md = '> > nested'
      expect(renderMarkdown(md)).toBe(
        '<blockquote><blockquote><p>nested</p></blockquote></blockquote>',
      )
    })
  })

  describe('horizontal rule', () => {
    it('should render hr', () => {
      expect(renderMarkdown('---')).toBe('<hr />')
      expect(renderMarkdown('***')).toBe('<hr />')
    })
  })

  describe('paragraphs', () => {
    it('should render paragraph', () => {
      expect(renderMarkdown('hello world')).toBe('<p>hello world</p>')
    })

    it('should handle multiple paragraphs', () => {
      const md = 'first\n\nsecond'
      expect(renderMarkdown(md)).toBe('<p>first</p><p>second</p>')
    })

    it('should handle line breaks in paragraph', () => {
      const md = 'line1\nline2'
      expect(renderMarkdown(md)).toBe('<p>line1<br />line2</p>')
    })
  })

  describe('complex documents', () => {
    it('should render release notes style markdown', () => {
      const md = `# v1.0.0

## Features
- New **player** UI
- Support for ".flac" files

## Fixes
> Fixed memory leak`
      const result = renderMarkdown(md)
      expect(result).toContain('<h1>v1.0.0</h1>')
      expect(result).toContain('<h2>Features</h2>')
      expect(result).toContain('<li>New <strong>player</strong> UI</li>')
      expect(result).toContain('<li>Support for &quot;.flac&quot; files</li>')
      expect(result).toContain('<blockquote><p>Fixed memory leak</p></blockquote>')
    })
  })

  describe('tables', () => {
    it('should render a basic table', () => {
      const md = `| 平台 | 文件 |
|------|------|
| Windows | .exe |
| macOS | .dmg |`
      const result = renderMarkdown(md)
      expect(result).toBe(
        '<table><thead><tr><th>平台</th><th>文件</th></tr></thead>' +
          '<tbody><tr><td>Windows</td><td>.exe</td></tr>' +
          '<tr><td>macOS</td><td>.dmg</td></tr></tbody></table>',
      )
    })

    it('should render table with inline formatting in cells', () => {
      const md = `| Type | Command |
|------|---------|
| Run | \`npm start\` |`
      const result = renderMarkdown(md)
      expect(result).toContain('<th>Type</th>')
      expect(result).toContain('<th>Command</th>')
      expect(result).toContain('<td>Run</td>')
      expect(result).toContain('<td><code>npm start</code></td>')
    })

    it('should render table with alignment separators', () => {
      const md = `| Left | Center | Right |
|:-----|:------:|------:|
| a | b | c |`
      const result = renderMarkdown(md)
      expect(result).toContain('<th>Left</th>')
      expect(result).toContain('<th>Center</th>')
      expect(result).toContain('<th>Right</th>')
      expect(result).toContain('<td>a</td>')
      expect(result).toContain('<td>b</td>')
      expect(result).toContain('<td>c</td>')
    })

    it('should not treat a single pipe line as table without separator', () => {
      const md = '| not a table'
      const result = renderMarkdown(md)
      expect(result).not.toContain('<table>')
    })

    it('should render table surrounded by other elements', () => {
      const md = `## Download

| OS | File |
|----|-----|
| Win | .msi |

Done.`
      const result = renderMarkdown(md)
      expect(result).toContain('<h2>Download</h2>')
      expect(result).toContain('<table>')
      expect(result).toContain('<td>.msi</td>')
      expect(result).toContain('<p>Done.</p>')
    })
  })
})
