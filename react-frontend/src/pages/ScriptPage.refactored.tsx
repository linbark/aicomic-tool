/**
 * ScriptPage - 重构版本
 * 使用拆分后的组件，提高可维护性
 */

import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import api from '../api/client'
import type { EpisodeRead, ShotRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'
import { EpisodeList } from '../components/script/EpisodeList'
import { EpisodeEditor } from '../components/script/EpisodeEditor'
import { SceneEditor } from '../components/script/SceneEditor'
import { ShotEditor } from '../components/script/ShotEditor'
import { DebugWindow } from '../components/script/DebugWindow'
import { Button } from '../components/ui/Button'
import { Input } from '../components/ui/Input'
import type { Selected, ChatRunUi, DebugLog, BusyState, DeletingState } from '../components/script/types'
import { now, lsSet, extractErrorMessage } from '../components/script/utils'
import { panelStyle } from '../styles/shared'

export function ScriptPage() {
  const { projects, projectId, setProjectId, refreshProjects } = useProjectSelection()
  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [selected, setSelected] = useState<Selected>({ kind: 'none' })

  // Editor states
  const [episodeText, setEpisodeText] = useState('')
  const [sceneText, setSceneText] = useState('')
  const [shotDraft, setShotDraft] = useState<Partial<ShotRead> | null>(null)

  const [busy, setBusy] = useState<BusyState>(null)
  const [error, setError] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<DeletingState>(null)
  const [showCreateProject, setShowCreateProject] = useState(false)
  const [createProjectName, setCreateProjectName] = useState('')
  const [createProjectDesc, setCreateProjectDesc] = useState('')
  const [creatingProject, setCreatingProject] = useState(false)

  const [execRun, setExecRun] = useState<ChatRunUi | null>(null)
  const [execPollPaused, setExecPollPaused] = useState(false)
  const [execBusy, setExecBusy] = useState(false)
  const [interruptKind, setInterruptKind] = useState<string | null>(null)
  const [uiNowMs, setUiNowMs] = useState(() => Date.now())
  const [rawAssetsVisualDnaText, setRawAssetsVisualDnaText] = useState<string>('')
  const [rawSplitEpisodesText, setRawSplitEpisodesText] = useState<string>('')
  const lastAssetsRawTsRef = useRef(0)
  const lastSplitRawTsRef = useRef(0)

  // Debug window
  const [debugLogs, setDebugLogs] = useState<DebugLog[]>([])
  const [showDebugWindow, setShowDebugWindow] = useState(false)
  const fetchedLogIdsRef = useRef<Set<string>>(new Set())
  const lastFinalTsRef = useRef(0)
  const lastInterruptTsRef = useRef(0)
  const [autoScrollEnabled, setAutoScrollEnabled] = useState(true)
  const [hasNewLogs, setHasNewLogs] = useState(false)

  const episodeDirtyForIdRef = useRef<number | null>(null)
  const sceneDirtyForIdRef = useRef<number | null>(null)

  const selectedEpisode = useMemo(() => {
    if (selected.kind === 'episode') return episodes.find((e) => e.id === selected.episodeId) || null
    if (selected.kind === 'scene' || selected.kind === 'shot') return episodes.find((e) => e.id === selected.episodeId) || null
    return null
  }, [episodes, selected])

  const selectedScene = useMemo(() => {
    if (selected.kind !== 'scene' && selected.kind !== 'shot') return null
    const ep = episodes.find((e) => e.id === selected.episodeId)
    return ep?.scenes?.find((s) => s.id === selected.sceneId) || null
  }, [episodes, selected])

  const selectedShot = useMemo(() => {
    if (selected.kind !== 'shot') return null
    const ep = episodes.find((e) => e.id === selected.episodeId)
    const sc = ep?.scenes?.find((s) => s.id === selected.sceneId)
    return sc?.shots?.find((sh) => sh.id === selected.shotId) || null
  }, [episodes, selected])

  const refreshScript = useCallback(
    async (pid = projectId) => {
      if (!pid) return
      setBusy('load')
      setError(null)
      try {
        const res = await api.getScript(pid)
        setEpisodes(res.data || [])
      } catch (e: any) {
        setError(extractErrorMessage(e, '加载失败'))
      } finally {
        setBusy(null)
      }
    },
    [projectId]
  )

  useEffect(() => {
    if (projectId) refreshScript(projectId).catch(() => {})
    else setEpisodes([])
  }, [projectId, refreshScript])

  // Sync editors when selection changes
  useEffect(() => {
    if (selected.kind === 'episode') {
      const ep = episodes.find((e) => e.id === selected.episodeId)
      setEpisodeText(String(ep?.description || ''))
      setSceneText('')
      setShotDraft(null)
      setInterruptKind(null)
      setRawAssetsVisualDnaText('')
      setRawSplitEpisodesText('')
      lastAssetsRawTsRef.current = 0
      lastSplitRawTsRef.current = 0
      if (ep?.last_exec_run_id) {
        setExecRun({
          runId: String(ep.last_exec_run_id),
          status: (ep.exec_status as any) || 'queued',
          steps: [],
          startedAtMs: Date.now(),
        })
      } else {
        setExecRun(null)
      }
      return
    }
    if (selected.kind === 'scene') {
      setEpisodeText('')
      const sc = selectedScene
      setSceneText(String(sc?.description || ''))
      setShotDraft(null)
      setExecRun(null)
      setInterruptKind(null)
      return
    }
    if (selected.kind === 'shot') {
      setEpisodeText('')
      setSceneText('')
      setShotDraft(selectedShot ? { ...selectedShot } : null)
      setExecRun(null)
      setInterruptKind(null)
      return
    }
    setEpisodeText('')
    setSceneText('')
    setShotDraft(null)
    setExecRun(null)
    setInterruptKind(null)
  }, [selected, episodes, projectId, selectedScene, selectedShot])

  useEffect(() => {
    if (!projectId) return
    if (!execRun?.runId) return
    if (execPollPaused) return
    let alive = true
    const pid = projectId
    const runId = execRun.runId

    async function tick() {
      if (!alive) return
      try {
        const stagesRes = await api.listRunStages(pid, runId)
        const stageMetas = ((stagesRes.data as any)?.stages || []) as { name: string; timestamp: number }[]
        const stageList = stageMetas.map((s) => s.name)
        const stageSet = new Set(stageList)
        const stageTs = new Map(stageMetas.map((s) => [s.name, s.timestamp]))

        if (stageSet.has('episode_assets_visual_dna.raw')) {
          const ts = Number(stageTs.get('episode_assets_visual_dna.raw') || 0)
          if (ts > lastAssetsRawTsRef.current) {
            lastAssetsRawTsRef.current = ts
            api
              .getRunStage(pid, runId, 'episode_assets_visual_dna.raw')
              .then((res) => {
                if (!alive) return
                const data = (res.data as any)?.data || {}
                const text = typeof data?.text === 'string' ? data.text : ''
                if (text) setRawAssetsVisualDnaText(text)
              })
              .catch(() => {})
          }
        }
        if (stageSet.has('episode_split_episodes.raw')) {
          const ts = Number(stageTs.get('episode_split_episodes.raw') || 0)
          if (ts > lastSplitRawTsRef.current) {
            lastSplitRawTsRef.current = ts
            api
              .getRunStage(pid, runId, 'episode_split_episodes.raw')
              .then((res) => {
                if (!alive) return
                const data = (res.data as any)?.data || {}
                const text = typeof data?.text === 'string' ? data.text : ''
                if (text) setRawSplitEpisodesText(text)
              })
              .catch(() => {})
          }
        }

        const logStages = stageList.filter((s) => s.startsWith('log.'))
        for (const logName of logStages) {
          if (fetchedLogIdsRef.current.has(logName)) continue
          fetchedLogIdsRef.current.add(logName)
          api.getRunStage(pid, runId, logName)
            .then((res) => {
              if (!alive) return
              const d = (res.data as any)?.data || {}
              setDebugLogs((prev) => {
                const ts =
                  typeof d?.ts_ms === 'number'
                    ? Number(d.ts_ms)
                    : typeof d?.at_ms === 'number'
                      ? Number(d.at_ms)
                      : d.timestamp
                        ? d.timestamp * 1000
                        : Date.now()
                const stage = d?.stage ? String(d.stage) : ''
                const summary = d?.summary ? String(d.summary) : d?.text ? String(d.text) : ''
                const text = stage ? `[${stage}] ${summary}` : summary
                const newLog = { id: logName, ts, level: d.level || 'INFO', text }
                const next = [...prev, newLog]
                next.sort((a, b) => a.ts - b.ts)
                return next
              })
            })
            .catch(() => {
              fetchedLogIdsRef.current.delete(logName)
            })
        }

        if (stageSet.has('chat.status')) {
          const st = await api.getRunStage(pid, runId, 'chat.status')
          const sd = (st.data as any)?.data || {}
          const s = sd?.status
          const atMs = typeof sd?.at_ms === 'number' ? Number(sd.at_ms) : null
          const curIdx = typeof sd?.current_step_index === 'number' ? Number(sd.current_step_index) : null
          const curAk = sd?.current_action_key ? String(sd.current_action_key) : null
          if (s && alive) {
            setExecRun((prev) => {
              if (!prev) return prev
              const next: any = {
                ...prev,
                status: s,
                currentStepIndex: curIdx,
                currentActionKey: curAk,
                lastAtMs: atMs != null ? atMs : prev.lastAtMs,
              }
              if (curIdx != null && prev.steps.length) {
                const steps = prev.steps.slice()
                for (let i = 0; i < steps.length; i++) {
                  const cur = steps[i]
                  if (!cur) continue
                  if (i < curIdx) steps[i] = { ...cur, status: 'done' }
                  else if (i === curIdx) steps[i] = { ...cur, status: 'running', action_key: curAk || cur.action_key }
                  else steps[i] = { ...cur, status: 'pending' }
                }
                next.steps = steps
              }
              return next
            })
          }
        }

        if (stageSet.has('chat.plan')) {
          const pl = await api.getRunStage(pid, runId, 'chat.plan')
          const plan = (pl.data as any)?.data?.plan
          const stepsArr = Array.isArray(plan?.steps) ? plan.steps : []
          if (alive && stepsArr.length) {
            setExecRun((prev) => {
              if (!prev) return prev
              if (prev.steps && prev.steps.length) return prev
              return {
                ...prev,
                steps: stepsArr.map((s: any, idx: number) => ({
                  index: idx,
                  action_key: String(s?.action_key || 'unknown'),
                  why: s?.why ? String(s.why) : null,
                  status: 'pending',
                })),
              }
            })
          }
        }

        const stepStartRe = /^chat\.step\.(\d+)\.start$/
        const stepEndRe = /^chat\.step\.(\d+)\.end$/
        for (const name of stageSet) {
          let m = String(name).match(stepStartRe)
          if (m) {
            const idx = Number(m[1])
            if (Number.isFinite(idx)) {
              setExecRun((prev) => {
                if (!prev) return prev
                const steps = prev.steps.slice()
                const cur = steps[idx]
                if (cur && cur.status === 'pending') steps[idx] = { ...cur, status: 'running' }
                return { ...prev, steps }
              })
            }
            continue
          }
          m = String(name).match(stepEndRe)
          if (m) {
            const idx = Number(m[1])
            if (!Number.isFinite(idx)) continue
            const ed = await api.getRunStage(pid, runId, String(name))
            const data = (ed.data as any)?.data || {}
            const ms = Number(data?.ms || 0)
            const outputPreview = data?.output_preview ? String(data.output_preview) : ''
            if (!alive) return
            setExecRun((prev) => {
              if (!prev) return prev
              const steps = prev.steps.slice()
              const cur = steps[idx]
              if (cur) steps[idx] = { ...cur, status: 'done', ms: Number.isFinite(ms) ? ms : undefined, output_preview: outputPreview }
              return { ...prev, steps }
            })
          }
        }

        for (const n of stageSet) {
          const name = String(n)
          const m = name.match(/^chat\.step\.(\d+)\.error$/)
          if (!m) continue
          const idx = Number(m[1])
          if (!Number.isFinite(idx)) continue
          const erd = await api.getRunStage(pid, runId, name)
          const msg = (erd.data as any)?.data?.error
          if (alive) {
            setExecRun((prev) => (prev ? { ...prev, status: 'error', error: String(msg || 'step_error') } : prev))
          }
        }
        if (stageSet.has('chat.error')) {
          const er = await api.getRunStage(pid, runId, 'chat.error')
          const msg = (er.data as any)?.data?.error
          if (alive) setExecRun((prev) => (prev ? { ...prev, status: 'error', error: String(msg || 'error') } : prev))
        }

        if (stageSet.has('chat.interrupt')) {
          const ts = Number(stageTs.get('chat.interrupt') || 0)
          if (ts > lastInterruptTsRef.current) {
            lastInterruptTsRef.current = ts
            const it = await api.getRunStage(pid, runId, 'chat.interrupt')
            const d = (it.data as any)?.data || {}
            const k = d?.kind ? String(d.kind) : null
            if (alive) setInterruptKind(k)
            refreshScript(pid).catch(() => {})
          }
        } else {
          if (alive) setInterruptKind(null)
        }

        if (stageSet.has('chat.final')) {
          const finalTs = Number(stageTs.get('chat.final') || 0)
          if (finalTs > lastFinalTsRef.current) {
            lastFinalTsRef.current = finalTs
            if (alive) {
              setExecRun((prev) => (prev ? { ...prev, status: 'done', steps: prev.steps.map((x) => ({ ...x, status: 'done' })) } : prev))
              refreshScript(pid).catch(() => {})
            }
          }
        }
      } catch (e: any) {
        console.warn('[poll tick error]', e?.message || e)
      }
    }

    const timer = window.setInterval(() => {
      tick().catch(() => {})
    }, 800)
    tick().catch(() => {})
    return () => {
      alive = false
      window.clearInterval(timer)
    }
  }, [projectId, execRun?.runId, execPollPaused, refreshScript])

  useEffect(() => {
    if (!execRun?.runId) return
    if (execRun.status !== 'running' && execRun.status !== 'queued' && execRun.status !== 'paused') return
    if (execPollPaused) return
    const timer = window.setInterval(() => setUiNowMs(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [execRun?.runId, execRun?.status, execPollPaused])


  useEffect(() => {
    if (autoScrollEnabled && debugLogs.length > 0) {
      setHasNewLogs(false)
    } else if (!autoScrollEnabled) {
      setHasNewLogs(true)
    }
  }, [debugLogs, autoScrollEnabled])

  // Action handlers
  const saveEpisode = useCallback(async () => {
    if (!projectId || selected.kind !== 'episode') return
    setBusy('save_episode')
    setError(null)
    try {
      episodeDirtyForIdRef.current = selected.episodeId
      await api.updateEpisode(selected.episodeId, { description: episodeText })
      await refreshScript(projectId)
      setSelected({ kind: 'episode', episodeId: selected.episodeId })
    } catch (e: any) {
      const isLocked =
        (e as any)?.response?.status === 409 &&
        String((e as any)?.response?.data?.detail || '').toLowerCase().includes('locked')
      if (isLocked) {
        setError('本集已锁定（执行后自动锁定），不能再修改剧本。')
        await refreshScript(projectId)
        setSelected({ kind: 'episode', episodeId: selected.episodeId })
      } else {
        setError(extractErrorMessage(e, '保存失败'))
      }
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, episodeText, refreshScript])

  const saveScene = useCallback(async () => {
    if (!projectId || selected.kind !== 'scene' || !selectedScene) return
    setBusy('save_scene')
    setError(null)
    try {
      sceneDirtyForIdRef.current = selectedScene.id
      await api.updateScene(selectedScene.id, { description: sceneText })
      await refreshScript(projectId)
      setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selectedScene.id })
    } catch (e: any) {
      setError(extractErrorMessage(e, '保存失败'))
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, selectedScene, sceneText, refreshScript])

  const saveShot = useCallback(async () => {
    if (!projectId || selected.kind !== 'shot' || !selectedShot || !shotDraft) return
    setBusy('save_shot')
    setError(null)
    try {
      await api.updateShot(selectedShot.id, {
        title: shotDraft.title,
        action_text: shotDraft.action_text,
        dialogue: shotDraft.dialogue,
        prompt: shotDraft.prompt,
      })
      await refreshScript(projectId)
      setSelected({ kind: 'shot', episodeId: selected.episodeId, sceneId: selected.sceneId, shotId: selectedShot.id })
    } catch (e: any) {
      setError(extractErrorMessage(e, '保存失败'))
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, selectedShot, shotDraft, refreshScript])

  const createEpisode = useCallback(async () => {
    if (!projectId) return
    setBusy('create_episode')
    setError(null)
    try {
      await api.createEpisode(projectId, { title: `第${episodes.length + 1}集` })
      await refreshScript(projectId)
    } catch (e: any) {
      setError(extractErrorMessage(e, '新建集失败'))
    } finally {
      setBusy(null)
    }
  }, [projectId, episodes.length, refreshScript])

  const createProject = useCallback(async () => {
    const name = (createProjectName || '').trim()
    const desc = (createProjectDesc || '').trim()
    if (!name) {
      setError('请输入项目名称')
      return
    }
    setCreatingProject(true)
    setError(null)
    try {
      const res = await api.createProject({ name, description: desc || null })
      const pid = String((res.data as any)?.id || '').trim()
      await refreshProjects()
      if (pid) setProjectId(pid)
      setShowCreateProject(false)
      setCreateProjectName('')
      setCreateProjectDesc('')
    } catch (e: any) {
      setError(extractErrorMessage(e, '新建项目失败'))
    } finally {
      setCreatingProject(false)
    }
  }, [createProjectName, createProjectDesc, refreshProjects, setProjectId])

  const deleteCurrentProject = useCallback(async () => {
    if (!projectId) return
    if (!window.confirm('确定删除当前项目？此操作不可恢复。')) return
    setDeleting('project')
    setError(null)
    try {
      await api.deleteProject(projectId)
      await refreshProjects()
    } catch (e: any) {
      setError(extractErrorMessage(e, '删除项目失败'))
    } finally {
      setDeleting(null)
    }
  }, [projectId, refreshProjects])

  const deleteCurrentEpisode = useCallback(async () => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const epId = selected.episodeId
    if (!window.confirm('确定删除当前集？此操作不可恢复。')) return
    setDeleting('episode')
    setError(null)
    try {
      await api.deleteEpisode(epId)
      await refreshScript(projectId)
      setSelected({ kind: 'none' })
    } catch (e: any) {
      setError(extractErrorMessage(e, '删除集失败'))
    } finally {
      setDeleting(null)
    }
  }, [projectId, selected, refreshScript])

  const createScene = useCallback(async () => {
    if (!projectId || selected.kind !== 'episode') return
    setBusy('create_scene')
    setError(null)
    try {
      await api.createScene(selected.episodeId, { title: `场${(selectedEpisode?.scenes || []).length + 1}` })
      await refreshScript(projectId)
    } catch (e: any) {
      setError(extractErrorMessage(e, '新建场失败'))
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, selectedEpisode, refreshScript])

  const createShot = useCallback(async () => {
    if (!projectId || selected.kind !== 'scene' || !selectedScene) return
    setBusy('create_shot')
    setError(null)
    try {
      await api.createShot(selectedScene.id, { title: `镜头${(selectedScene.shots || []).length + 1}`, action_text: '' })
      await refreshScript(projectId)
    } catch (e: any) {
      setError(extractErrorMessage(e, '新建镜头失败'))
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, selectedScene, refreshScript])

  const executeEpisode = useCallback(async () => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const text = String(episodeText || '').trim()
    if (!text) return
    if (selectedEpisode?.script_locked) {
      setError('本集已锁定（执行后自动锁定），不能再次执行。')
      return
    }
    setExecBusy(true)
    setError(null)
    try {
      try {
        await api.updateEpisode(selected.episodeId, { description: episodeText })
      } catch (e: any) {
        const isLocked =
          (e as any)?.response?.status === 409 &&
          String((e as any)?.response?.data?.detail || '').toLowerCase().includes('locked')
        if (isLocked) {
          setError('本集已锁定（执行后自动锁定），不能再次执行。')
          await refreshScript(projectId)
          setSelected({ kind: 'episode', episodeId: selected.episodeId })
          return
        }
        throw e
      }
      const runId =
        typeof window !== 'undefined' && (window as any).crypto && (window as any).crypto.randomUUID
          ? (window as any).crypto.randomUUID().replace(/-/g, '')
          : (Math.random().toString(16).slice(2) + Date.now().toString(16)).slice(0, 32)
      lsSet('aicomic.lastRunId', runId)
      const res = await api.aiEpisodeExecuteActAsync({
        project_id: projectId,
        episode_id: selected.episodeId,
        script_text: episodeText,
        run_id: runId,
      })
      const rid = String(res.data?.run_id || runId)
      setExecRun({
        runId: rid,
        status: 'queued',
        steps: [],
        startedAtMs: now(),
        lastAtMs: now(),
        currentStepIndex: null,
        currentActionKey: null,
      })
      setExecPollPaused(false)
      setInterruptKind(null)
      lastFinalTsRef.current = 0
      lastInterruptTsRef.current = 0
      setDebugLogs([])
      fetchedLogIdsRef.current.clear()
      await refreshScript(projectId)
    } catch (e: any) {
      const isLocked =
        (e as any)?.response?.status === 409 &&
        String((e as any)?.response?.data?.detail || '').toLowerCase().includes('locked')
      if (isLocked) {
        setError('本集已锁定（执行后自动锁定），不能再次执行。')
        await refreshScript(projectId)
        setSelected({ kind: 'episode', episodeId: selected.episodeId })
      } else {
        setError(extractErrorMessage(e, '执行失败'))
      }
    } finally {
      setExecBusy(false)
    }
  }, [episodeText, projectId, refreshScript, selected, selectedEpisode])

  const confirmExec = useCallback(
    async (decision: 'confirmed' | 'regenerate' | 'rejected', artifacts?: Record<string, unknown>) => {
      if (!projectId) return
      if (selected.kind !== 'episode') return
      const runId = execRun?.runId || String(selectedEpisode?.last_exec_run_id || '')
      if (!runId) return
      setExecBusy(true)
      setError(null)
      try {
        await api.aiEpisodeExecuteConfirm(selected.episodeId, { decision, artifacts: artifacts || null, run_id: runId })
        setExecPollPaused(false)
        setInterruptKind(null)
        await refreshScript(projectId)
      } catch (e: any) {
        setError(extractErrorMessage(e, '提交确认失败'))
      } finally {
        setExecBusy(false)
      }
    },
    [execRun?.runId, projectId, refreshScript, selected, selectedEpisode?.last_exec_run_id, selectedEpisode?.id]
  )

  const handlePauseExecPoll = useCallback(() => {
    setExecPollPaused(true)
  }, [])

  const handleResumeExecPoll = useCallback(() => {
    setExecPollPaused(false)
  }, [])

  const handleForceRefreshExec = useCallback(async () => {
    if (!projectId) return
    const runId = execRun?.runId || ''
    if (!runId) return
    try {
      const stagesRes = await api.listRunStages(projectId, runId)
      const stageMetas = ((stagesRes.data as any)?.stages || []) as { name: string }[]
      const stageSet = new Set(stageMetas.map((s) => String(s?.name || '')).filter((x) => x))
      if (stageSet.has('chat.interrupt')) {
        const it = await api.getRunStage(projectId, runId, 'chat.interrupt')
        const d = (it.data as any)?.data || {}
        setInterruptKind(d?.kind ? String(d.kind) : null)
      }
      await refreshScript(projectId)
    } catch (e: any) {
      setError(extractErrorMessage(e, '刷新失败'))
    }
  }, [execRun?.runId, projectId, refreshScript])

  return (
    <div style={styles.page}>
      <div style={styles.topbar}>
        <div style={{ fontWeight: 700 }}>剧本</div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <select
            value={projectId ?? ''}
            onChange={(e) => setProjectId(e.target.value || null)}
            style={styles.select}
            aria-label="选择项目"
          >
            <option value="" disabled>
              选择项目…
            </option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <Button onClick={() => projectId && refreshScript(projectId).catch(() => {})} disabled={!projectId || busy === 'load'}>
            {busy === 'load' ? '加载中…' : '刷新'}
          </Button>
          <Button variant="primary" onClick={() => setShowCreateProject(true)}>
            + 新建项目
          </Button>
          <Button variant="primary" onClick={() => createEpisode().catch(() => {})} disabled={!projectId || busy === 'create_episode'}>
            + 新建集
          </Button>
          <Button onClick={() => deleteCurrentProject().catch(() => {})} disabled={!projectId || deleting === 'project'}>
            {deleting === 'project' ? '删除中…' : '删除项目'}
          </Button>
        </div>
      </div>

      {showCreateProject ? (
        <div style={{ ...panelStyle, marginBottom: 12 }}>
          <div style={{ ...styles.panelHeader, marginBottom: 12 }}>
            <div style={{ fontWeight: 700 }}>新建项目</div>
            <div style={{ display: 'flex', gap: 8 }}>
              <Button onClick={() => setShowCreateProject(false)} disabled={creatingProject}>
                取消
              </Button>
              <Button variant="primary" onClick={() => createProject().catch(() => {})} disabled={creatingProject || !createProjectName.trim()}>
                {creatingProject ? '创建中…' : '创建'}
              </Button>
            </div>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
            <div>
              <div style={styles.labelRow}>
                <div style={styles.label}>项目名称</div>
              </div>
              <Input
                value={createProjectName}
                onChange={(e) => setCreateProjectName(e.target.value)}
                placeholder="例如：西游记"
                aria-label="项目名称"
              />
            </div>
            <div>
              <div style={styles.labelRow}>
                <div style={styles.label}>项目简介（可选）</div>
              </div>
              <Input
                value={createProjectDesc}
                onChange={(e) => setCreateProjectDesc(e.target.value)}
                placeholder="一句话描述故事/风格…"
                aria-label="项目简介"
              />
            </div>
          </div>
        </div>
      ) : null}

      {error ? (
        <div style={{ color: '#f87171', fontSize: 12, marginBottom: 10 }} role="alert" aria-live="assertive">
          {error}
        </div>
      ) : null}

      <div style={styles.body}>
        <EpisodeList
          episodes={episodes}
          selected={selected}
          selectedEpisodeId={selectedEpisode?.id || null}
          selectedSceneId={selectedScene?.id || null}
          onSelectEpisode={(episodeId) => setSelected({ kind: 'episode', episodeId })}
          onSelectScene={(episodeId, sceneId) => setSelected({ kind: 'scene', episodeId, sceneId })}
        />

        <div style={styles.main}>
          {selected.kind === 'none' ? <div style={styles.empty}>请选择一个 Episode 或 Scene</div> : null}

          {selected.kind === 'episode' && selectedEpisode ? (
            <EpisodeEditor
              episode={selectedEpisode}
              episodeText={episodeText}
              uiNowMs={uiNowMs}
              execRun={execRun}
              execPollPaused={execPollPaused}
              interruptKind={interruptKind}
              execBusy={execBusy}
              busy={busy === 'save_episode'}
              deleting={deleting === 'episode'}
              onEpisodeTextChange={setEpisodeText}
              onSave={saveEpisode}
              onCreateScene={createScene}
              onDelete={deleteCurrentEpisode}
              onExecute={executeEpisode}
              onPauseExecPoll={handlePauseExecPoll}
              onResumeExecPoll={handleResumeExecPoll}
              onForceRefreshExec={handleForceRefreshExec}
              onConfirmExec={confirmExec}
              rawAssetsVisualDnaText={rawAssetsVisualDnaText}
              rawSplitEpisodesText={rawSplitEpisodesText}
            />
          ) : null}

          {selected.kind === 'scene' && selectedScene ? (
            <SceneEditor
              scene={selectedScene}
              sceneText={sceneText}
              busy={busy === 'save_scene'}
              onSceneTextChange={setSceneText}
              onCreateShot={createShot}
              onSave={saveScene}
              onSelectShot={(shotId) => setSelected({ kind: 'shot', episodeId: selected.episodeId, sceneId: selected.sceneId, shotId })}
              selectedShotId={selectedShot?.id || null}
            />
          ) : null}

          {selected.kind === 'shot' && selectedShot && shotDraft ? (
            <ShotEditor shot={selectedShot} shotDraft={shotDraft} busy={busy === 'save_shot'} onDraftChange={setShotDraft} onSave={saveShot} />
          ) : null}
        </div>
      </div>

      <DebugWindow
        show={showDebugWindow}
        debugLogs={debugLogs}
        chatRun={execRun}
        autoScrollEnabled={autoScrollEnabled}
        hasNewLogs={hasNewLogs}
        onToggle={() => setShowDebugWindow(!showDebugWindow)}
        onClear={() => {
          setDebugLogs([])
          fetchedLogIdsRef.current.clear()
        }}
        onAutoScrollToggle={() => setAutoScrollEnabled(!autoScrollEnabled)}
        onScrollToBottom={() => {
          // 滚动逻辑在 DebugWindow 内部处理
        }}
      />
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: {
    padding: 14,
    color: 'rgba(255,255,255,0.92)',
  },
  topbar: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
    marginBottom: 12,
  },
  body: {
    display: 'grid',
    gridTemplateColumns: '320px 1fr',
    gap: 12,
    alignItems: 'start',
  },
  main: {},
  empty: {
    border: '1px dashed rgba(255,255,255,0.2)',
    borderRadius: 12,
    padding: 20,
    opacity: 0.7,
  },
  select: {
    background: 'rgba(0,0,0,0.25)',
    border: '1px solid rgba(255,255,255,0.14)',
    color: 'rgba(255,255,255,0.92)',
    borderRadius: 10,
    padding: '8px 10px',
    outline: 'none',
    cursor: 'pointer',
  },
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 10,
    marginBottom: 10,
  },
  labelRow: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 6,
  },
  label: {
    fontSize: 12,
    fontWeight: 700,
    opacity: 0.85,
  },
}
