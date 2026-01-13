/**
 * ScriptPage 工具函数
 */

export function now() {
  return Date.now()
}

export function trim(text: string, limit: number) {
  const t = String(text || '')
  return t.length > limit ? t.slice(0, limit) + '…' : t
}

export function lsGet(key: string): string | null {
  try {
    return window.localStorage.getItem(key)
  } catch {
    return null
  }
}

export function lsSet(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value)
  } catch {
    // ignore
  }
}

export function chatKey(projectId: string, episodeId: number) {
  return `aicomic.episode_chat.${projectId}.${episodeId}`
}
