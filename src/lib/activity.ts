const actionNames: Record<string, string> = {
  'scope.add': '添加目录',
  'scope.remove': '撤销目录',
  'fs.list': '浏览目录',
  'fs.read': '读取文件',
  'fs.write': '写入文件',
  'fs.search': '搜索文件',
  'fs.mutate': '修改目录',
  'server.start': '启动服务',
  'server.stop': '停止服务',
  'tunnel.start': '连接隧道',
  'token.rotate': '更新令牌',
  'workspace.list': '查询目录',
  'command.exec': '执行目录内命令',
  'scope.rename': '修改目录名称',
  'codex.exec': '调用本机 Codex',
  'pi.exec': '调用本机 Pi',
  'execution.start': '启动执行',
  'execution.end': '保存执行结果',
  'execution.list': '查询执行会话',
  'execution.get': '读取执行记录',
  'execution.output': '读取执行输出',
  'execution.cancel': '停止执行',
  'execution.review': '记录验收结果',
};

export function actionLabel(action: string) {
  return actionNames[action] ?? action;
}

/** Whether an audit entry concerns a workspace: targets are either its name or a path inside it. */
export function auditMatches(target: string, workspace: { name: string; path: string }) {
  if (!target) return false;
  if (target === workspace.name) return true;
  const root = workspace.path.replace(/\/+$/u, '');
  return target === root || target.startsWith(root + '/');
}
