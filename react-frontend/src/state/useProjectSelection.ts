import { useEffect, useState } from 'react'
import api from '../api/client'
import type { ProjectBase } from '../api/types'

const LS_KEY = 'aicomic.projectId'

export function useProjectSelection() {
  const [projects, setProjects] = useState<ProjectBase[]>([])
  const [projectId, setProjectIdState] = useState<number | null>(() => {
    const raw = typeof window !== 'undefined' ? window.localStorage.getItem(LS_KEY) : null
    const v = raw ? Number(raw) : NaN
    return Number.isFinite(v) ? v : null
  })

  function setProjectId(next: number | null) {
    setProjectIdState(next)
    if (typeof window === 'undefined') return
    if (next == null) window.localStorage.removeItem(LS_KEY)
    else window.localStorage.setItem(LS_KEY, String(next))
  }

  async function refreshProjects() {
    const res = await api.getProjects()
    const list = res.data || []
    setProjects(list)
    // 若当前 projectId 不存在（或为空），自动选择第一个，并同步到 localStorage
    const next =
      projectId != null && list.some((p) => p.id === projectId) ? projectId : list.length > 0 ? list[0].id : null
    setProjectId(next)
  }

  useEffect(() => {
    let alive = true
    ;(async () => {
      try {
        const res = await api.getProjects()
        if (!alive) return
        const list = res.data || []
        setProjects(list)
        // 初次加载：若 localStorage 里无有效值，选择第一个（并持久化）
        const next =
          projectId != null && list.some((p) => p.id === projectId) ? projectId : list.length > 0 ? list[0].id : null
        setProjectId(next)
      } catch {
        // ignore
      }
    })().catch(() => {
      // ignore
    })
    return () => {
      alive = false
    }
  }, [])

  return { projects, projectId, setProjectId, refreshProjects }
}


