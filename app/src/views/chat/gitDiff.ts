/**
 * 统一 diff 文本的行分类：返回该行的修饰类名（上下文行返回空串）。
 * 判 `+++`/`---` 必须先于 `+`/`-`——文件头行也以加减号开头。
 */
export function diffLineClass(line: string): string {
  if (
    line.startsWith("+++") ||
    line.startsWith("---") ||
    line.startsWith("diff --git") ||
    line.startsWith("index ") ||
    line.startsWith("new file mode") ||
    line.startsWith("deleted file mode") ||
    line.startsWith("Binary files")
  ) {
    return "is-meta";
  }
  if (line.startsWith("@@")) return "is-hunk";
  if (line.startsWith("+")) return "is-add";
  if (line.startsWith("-")) return "is-del";
  return "";
}

/** 变更文件树的节点：目录带 children，文件带 file（原始路径与状态字母）。 */
export type FileTreeNode = {
  name: string;
  path: string;
  children?: FileTreeNode[];
  file?: { path: string; status: string };
};

/**
 * 把扁平的变更文件路径（`/` 分隔）聚成目录树：
 * 单子目录链折叠成一个节点（`src/components` 形态），目录排在文件前、同级按名称排序。
 */
export function buildFileTree(files: { path: string; status: string }[]): FileTreeNode[] {
  const roots: FileTreeNode[] = [];
  for (const file of files) {
    const segments = file.path.split("/");
    let siblings = roots;
    let prefix = "";
    for (let i = 0; i < segments.length; i++) {
      const name = segments[i];
      prefix = prefix ? `${prefix}/${name}` : name;
      const isFile = i === segments.length - 1;
      let node = siblings.find((n) => n.name === name && !n.children === isFile);
      if (!node) {
        node = isFile ? { name, path: prefix, file } : { name, path: prefix, children: [] };
        siblings.push(node);
      }
      if (!isFile) siblings = node.children!;
    }
  }

  // 单子目录链折叠：a/b/c 且每层只有唯一子目录时合成一个 a/b/c 节点。
  const collapse = (nodes: FileTreeNode[]): FileTreeNode[] =>
    nodes.map((node) => {
      if (!node.children) return node;
      let collapsed: FileTreeNode = { ...node, children: collapse(node.children) };
      while (collapsed.children!.length === 1 && collapsed.children![0].children) {
        const child = collapsed.children![0];
        collapsed = {
          name: `${collapsed.name}/${child.name}`,
          path: child.path,
          children: child.children,
        };
      }
      return collapsed;
    });

  const byName = (a: FileTreeNode, b: FileTreeNode) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
  const sortNodes = (nodes: FileTreeNode[]): FileTreeNode[] =>
    nodes
      .map((node) => (node.children ? { ...node, children: sortNodes(node.children) } : node))
      .sort((a, b) => Number(Boolean(b.children)) - Number(Boolean(a.children)) || byName(a, b));

  return sortNodes(collapse(roots));
}

/** 路径比较用的统一形态：正斜杠、去尾斜杠、小写（Windows 路径不分大小写，大小写不同的
 *  同名文件在别的平台上几乎碰不到）。 */
function normPath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/**
 * 本会话里编辑类工具碰过的文件（归一化的绝对路径）。只认插件解析出结构化详情的工具，
 * 目前只有 claude 的 Edit/Write 带这个；其他 agent 返回空数组，面板据此不出过滤开关。
 * 相对路径按会话 cwd 补全。
 */
export function sessionTouchedPaths(items: readonly { type: string; detail?: { kind: string; path?: string | null } | null }[], cwd: string | null): string[] {
  const out = new Set<string>();
  for (const item of items) {
    if (item.type !== "tool_use" || !item.detail) continue;
    const { kind, path } = item.detail;
    if ((kind !== "edit" && kind !== "write") || !path) continue;
    const absolute = /^([a-zA-Z]:)?[\\/]/.test(path) || !cwd ? path : `${cwd}/${path}`;
    out.add(normPath(absolute));
  }
  return [...out];
}

/**
 * 变更清单里的文件（相对仓库根）是不是本会话改的。前端不知道仓库根在哪：
 * 面板 cwd 可能是仓库里的子目录。所以反过来验：绝对路径以 `/<rel>` 结尾，且剩下的
 * 前缀是面板 cwd 本身或它的上级目录，那个前缀就是仓库根。
 */
export function touchedByPath(touched: readonly string[], cwd: string, rel: string): boolean {
  const suffix = "/" + normPath(rel);
  const dir = normPath(cwd);
  return touched.some((path) => {
    if (!path.endsWith(suffix)) return false;
    const root = path.slice(0, path.length - suffix.length);
    return dir === root || dir.startsWith(root + "/");
  });
}
