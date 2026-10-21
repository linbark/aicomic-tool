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

export function extractErrorMessage(e: any, defaultMsg: string = '未知错误'): string {
  if (e?.response?.data?.detail) {
    return typeof e.response.data.detail === 'object'
      ? JSON.stringify(e.response.data.detail)
      : String(e.response.data.detail)
  }
  if (e?.message) {
    return String(e.message)
  }
  return defaultMsg
}
