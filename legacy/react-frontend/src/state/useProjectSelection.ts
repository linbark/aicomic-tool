import { useCallback, useEffect, useRef, useState } from 'react'
import api from '../api/client'
import type { ProjectBase } from '../api/types'

const LS_KEY = 'aicomic.projectId'

export function useProjectSelection() {
  const [projects, setProjects] = useState<ProjectBase[]>([])
  const [projectId, setProjectIdState] = useState<string | null>(() => {
    const raw = typeof window !== 'undefined' ? window.localStorage.getItem(LS_KEY) : null
    const v = (raw || '').trim()
    return v ? v : null
  })

  const projectIdRef = useRef<string | null>(projectId)
  useEffect(() => {
    projectIdRef.current = projectId
  }, [projectId])

  function setProjectId(next: string | null) {
    setProjectIdState(next)
    if (typeof window === 'undefined') return
    if (next == null) window.localStorage.removeItem(LS_KEY)
    else window.localStorage.setItem(LS_KEY, String(next))
  }

  const refreshProjects = useCallback(async () => {
    const res = await api.getProjects()
    const list = res.data || []
    setProjects(list)
    const cur = projectIdRef.current
    const next = cur != null && list.some((p) => p.id === cur) ? cur : list.length > 0 ? list[0].id : null
    setProjectId(next)
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return
    let alive = true
    let attempt = 0
    let timer: number | null = null

    const load = async () => {
      if (!alive) return
      try {
        await refreshProjects()
        attempt = 0
      } catch {
        attempt++
        timer = window.setTimeout(() => {
          load().catch(() => {})
        }, Math.min(1500, 200 + attempt * 100))
      }
    }

    const trigger = () => {
      attempt = 0
      if (timer != null) {
        window.clearTimeout(timer)
        timer = null
      }
      load().catch(() => {})
    }

    trigger()
    window.addEventListener('aicomic-api-base-url', trigger)
    window.addEventListener('focus', trigger)
    return () => {
      alive = false
      if (timer != null) window.clearTimeout(timer)
      window.removeEventListener('aicomic-api-base-url', trigger)
      window.removeEventListener('focus', trigger)
    }
  }, [refreshProjects])

  return { projects, projectId, setProjectId, refreshProjects }
}
