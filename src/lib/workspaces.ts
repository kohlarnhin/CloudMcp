const avatarColors = ['#343436', '#3a3a3c', '#414143', '#474749', '#38383a', '#3d3d3f', '#434345', '#4b4b4d', '#3b3b3d', '#454547'];

export function workspaceAvatar(name: string) {
  const trimmed = name.trim();
  let hash = 2166136261;
  for (const character of trimmed.normalize('NFC').toLowerCase()) {
    hash = Math.imul(hash ^ character.codePointAt(0)!, 16777619) >>> 0;
  }
  return { color: avatarColors[hash % avatarColors.length], initial: Array.from(trimmed)[0]?.toLocaleUpperCase() || '?' };
}

export function directoryName(path: string) {
  return path.split('/').filter(Boolean).at(-1) ?? 'workspace';
}
