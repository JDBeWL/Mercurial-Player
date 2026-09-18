// 测试环境只需要读源文件文本。这里声明最小的 node:fs 接口，
// 而不是把 "node" 加进 tsconfig 的 types —— 那会让 src 下的浏览器代码也能误用 node API。
declare module 'node:fs' {
  export function readFileSync(path: string, encoding: 'utf8'): string
}
