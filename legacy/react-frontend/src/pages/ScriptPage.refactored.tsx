/**
 * ScriptPage - 重构版本
 * 使用拆分后的组件，提高可维护性
 */

import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import api from '../api/client'
import type { EpisodeRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'
import { ScriptSidebar } from '../components/script/ScriptSidebar'
import { Step0_Script } from '../components/script/steps/Step0_Script'
import { Step1_Structure } from '../components/script/steps/Step1_Structure'
import { Step2_Assets } from '../components/script/steps/Step2_Assets'
import { DebugWindow } from '../components/script/DebugWindow'
import { Button } from '../components/ui/Button'
import { Input } from '../components/ui/Input'
import type { Selected, ChatRunUi, DebugLog, BusyState, DeletingState } from '../components/script/types'
import { now, lsGet, lsSet, extractErrorMessage } from '../components/script/utils'
import { panelStyle } from '../styles/shared'

export function ScriptPage() {
  const { projects, projectId, setProjectId, refreshProjects } = useProjectSelection()
  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [selected, setSelected] = useState<Selected>({ kind: 'none' })

  // Editor states
  const [episodeText, setEpisodeText] = useState('')

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
  const [execWaitingFor, setExecWaitingFor] = useState<string | null>(null)
  const [rawAssetsVisualDnaText, setRawAssetsVisualDnaText] = useState<string>('')
  const [rawOutlineText, setRawOutlineText] = useState<string>('')
  const lastAssetsRawTsRef = useRef(0)
  const lastOutlineRawTsRef = useRef(0)
  const selectedEpisodeIdRef = useRef<number | null>(null)

  // Debug window
  const [debugLogs, setDebugLogs] = useState<DebugLog[]>([])
  const [showDebugWindow, setShowDebugWindow] = useState(false)
  const fetchedLogIdsRef = useRef<Set<string>>(new Set())
  const lastFinalTsRef = useRef(0)
  const lastInterruptTsRef = useRef(0)
  const [autoScrollEnabled, setAutoScrollEnabled] = useState(true)
  const [hasNewLogs, setHasNewLogs] = useState(false)

  const episodeDirtyForIdRef = useRef<number | null>(null)
  const lastSelectionKeyRef = useRef<string>('')
  const lastUiStatusRef = useRef<string>('')
  const lastUiWaitingForRef = useRef<string>('')

  const pushUiLog = useCallback((level: string, text: string) => {
    setDebugLogs((prev) => {
      const ts = Date.now()
      const id = `ui.${ts}.${Math.random().toString(16).slice(2)}`
      const next = [...prev, { id, ts, level, text }]
      next.sort((a, b) => a.ts - b.ts)
      return next
    })
  }, [])

  const selectedEpisode = useMemo(() => {
    if (selected.kind === 'episode') return episodes.find((e) => e.id === selected.episodeId) || null
    return null
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
    const key =
      selected.kind === 'episode'
        ? `episode:${selected.episodeId}`
        : 'none'
    const selectionChanged = key !== lastSelectionKeyRef.current
    if (selectionChanged) lastSelectionKeyRef.current = key

    if (selected.kind === 'episode') {
      selectedEpisodeIdRef.current = selected.episodeId
      const ep = episodes.find((e) => e.id === selected.episodeId)
      if (selectionChanged) {
        setEpisodeText(String(ep?.description || ''))
        const cachedOutline = projectId ? lsGet(`aicomic.episode_outline_raw.${projectId}.${selected.episodeId}`) : null
        const cachedAssets = projectId ? lsGet(`aicomic.episode_assets_raw.${projectId}.${selected.episodeId}`) : null

        const artifacts = (ep as any)?.exec_artifacts
        const outlineFromArtifacts =
          artifacts && typeof artifacts === 'object' && artifacts !== null ? (artifacts as any).outline : null
        const assetsFromArtifacts =
          artifacts && typeof artifacts === 'object' && artifacts !== null ? (artifacts as any).assets_visual_dna : null

        const outlineText =
          typeof cachedOutline === 'string' && cachedOutline.trim()
            ? cachedOutline
            : outlineFromArtifacts && typeof outlineFromArtifacts === 'object'
              ? JSON.stringify(outlineFromArtifacts, null, 2)
              : ''
        const assetsText =
          typeof cachedAssets === 'string' && cachedAssets.trim()
            ? cachedAssets
            : assetsFromArtifacts && typeof assetsFromArtifacts === 'object'
              ? JSON.stringify(assetsFromArtifacts, null, 2)
              : ''

        setRawOutlineText(outlineText)
        setRawAssetsVisualDnaText(assetsText)
        lastAssetsRawTsRef.current = 0
        lastOutlineRawTsRef.current = 0
        lastFinalTsRef.current = 0
        lastInterruptTsRef.current = 0
        fetchedLogIdsRef.current.clear()
        setDebugLogs([])
        setExecWaitingFor(null)
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
      } else {
        if (ep?.last_exec_run_id) {
          const runId = String(ep.last_exec_run_id)
          setExecRun((prev) => {
            if (prev?.runId) return prev
            return { runId, status: (ep.exec_status as any) || 'queued', steps: [], startedAtMs: Date.now() }
          })
        }
      }
      return
    }
    
    if (selectionChanged) {
      selectedEpisodeIdRef.current = null
      setEpisodeText('')
      setExecRun(null)
      setExecWaitingFor(null)
    }
  }, [selected, episodes, projectId])

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
                if (text) {
                  setRawAssetsVisualDnaText(text)
                  const eid = selectedEpisodeIdRef.current
                  if (eid != null) lsSet(`aicomic.episode_assets_raw.${pid}.${eid}`, text)
                }
              })
              .catch(() => {})
          }
        }
        if (stageSet.has('episode_outline_generate.raw')) {
          const ts = Number(stageTs.get('episode_outline_generate.raw') || 0)
          if (ts > lastOutlineRawTsRef.current) {
            lastOutlineRawTsRef.current = ts
            api
              .getRunStage(pid, runId, 'episode_outline_generate.raw')
              .then((res) => {
                if (!alive) return
                const data = (res.data as any)?.data || {}
                const text = typeof data?.text === 'string' ? data.text : ''
                if (text) {
                  setRawOutlineText(text)
                  const eid = selectedEpisodeIdRef.current
                  if (eid != null) lsSet(`aicomic.episode_outline_raw.${pid}.${eid}`, text)
                }
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
          const waitingFor = sd?.waiting_for ? String(sd.waiting_for) : null
          if (alive && typeof s === 'string') {
            if (s !== lastUiStatusRef.current) {
              lastUiStatusRef.current = s
              if (s === 'done') pushUiLog('SUCCESS', '后台执行完成')
              if (s === 'error') pushUiLog('ERROR', '后台执行失败（可在上方查看错误详情）')
              if (s === 'timeout') pushUiLog('ERROR', '后台执行超时（可尝试重试或调整模型超时设置）')
            }
            if (s === 'paused' && waitingFor && waitingFor !== lastUiWaitingForRef.current) {
              lastUiWaitingForRef.current = waitingFor
              pushUiLog('INFO', `后台已暂停，等待确认：${waitingFor}（step=${curIdx ?? '-'} ${curAk || ''}）`)
            }
          }
          if (s === 'paused' && waitingFor) {
            setExecWaitingFor(waitingFor)
          } else if (s && s !== 'paused') {
            setExecWaitingFor(null)
          }
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
            refreshScript(pid).catch(() => {})
          }
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
  }, [projectId, execRun?.runId, execPollPaused, refreshScript, pushUiLog])

  useEffect(() => {
    if (autoScrollEnabled && debugLogs.length > 0) {
      setHasNewLogs(false)
    } else if (!autoScrollEnabled) {
      setHasNewLogs(true)
    }
  }, [debugLogs, autoScrollEnabled])

  const saveEpisode = useCallback(async () => {
    if (!projectId || selected.kind !== 'episode') return
    setBusy('save_episode')
    setError(null)
    try {
      episodeDirtyForIdRef.current = selected.episodeId
      await api.updateEpisode(selected.episodeId, { description: episodeText })
      await refreshScript(projectId)
      setSelected({ kind: 'episode', episodeId: selected.episodeId, step: 0 })
    } catch (e: any) {
      const isLocked =
        (e as any)?.response?.status === 409 &&
        String((e as any)?.response?.data?.detail || '').toLowerCase().includes('locked')
      if (isLocked) {
        setError('本集已锁定（执行后自动锁定），不能再修改剧本。')
        await refreshScript(projectId)
        setSelected({ kind: 'episode', episodeId: selected.episodeId, step: 0 })
      } else {
        setError(extractErrorMessage(e, '保存失败'))
      }
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, episodeText, refreshScript])

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

  const executeEpisode = useCallback(async () => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const text = String(episodeText || '').trim()
    if (!text) return // Don't run if empty

    setExecBusy(true)
    setError(null)
    try {
      pushUiLog('INFO', `执行请求准备中：status=${execRun?.status || 'none'} waiting=${execWaitingFor || '-'}`)
      pushUiLog('INFO', '开始执行：结构拆解（准备保存剧本并启动后端执行）')
      // Always update script before execution
      try {
        pushUiLog('INFO', '保存剧本到后端...')
        await api.updateEpisode(selected.episodeId, { description: episodeText })
        pushUiLog('SUCCESS', '保存剧本成功')
      } catch (e: any) {
        // Ignore lock errors if they still happen, but backend should be fixed now
        const isLocked =
          (e as any)?.response?.status === 409 &&
          String((e as any)?.response?.data?.detail || '').toLowerCase().includes('locked')
        if (isLocked) {
          // Just proceed, assume we can run anyway
          console.warn('Backend reported locked, but proceeding with execution per user request.')
          pushUiLog('WARN', '后端提示已锁定，仍尝试继续执行')
        } else {
           throw e
        }
      }

      const runId =
        typeof window !== 'undefined' && (window as any).crypto && (window as any).crypto.randomUUID
          ? (window as any).crypto.randomUUID().replace(/-/g, '')
          : (Math.random().toString(16).slice(2) + Date.now().toString(16)).slice(0, 32)
      lsSet('aicomic.lastRunId', runId)
      
      pushUiLog('INFO', `启动执行请求：run=${runId.slice(0, 8)}...`)
      const res = await api.aiEpisodeExecuteActAsync({
        project_id: projectId,
        episode_id: selected.episodeId,
        script_text: episodeText,
        run_id: runId,
      })
      const rid = String(res.data?.run_id || runId)
      pushUiLog('SUCCESS', `执行已启动：run=${rid.slice(0, 8)}...（开始轮询）`)
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
      lastFinalTsRef.current = 0
      lastInterruptTsRef.current = 0
      lastUiStatusRef.current = ''
      lastUiWaitingForRef.current = ''
      setDebugLogs([])
      fetchedLogIdsRef.current.clear()
      setExecBusy(false)
      await refreshScript(projectId)
    } catch (e: any) {
      setError(extractErrorMessage(e, '执行失败'))
      pushUiLog('ERROR', `执行失败：${extractErrorMessage(e, '') || 'unknown'}`)
    } finally {
      setExecBusy(false)
    }
  }, [episodeText, execRun?.status, execWaitingFor, projectId, refreshScript, selected, pushUiLog])

  // Auto-run logic: If entering Step 1, 2, or 3 and data is missing but script exists, trigger execution
  useEffect(() => {
    if (selected.kind !== 'episode' || !selectedEpisode) return
    if (execBusy || execRun?.status === 'running' || execRun?.status === 'queued' || execRun?.status === 'paused') return
    const st = String(selectedEpisode.exec_status || '')
    if (st.startsWith('waiting_')) return
    if (!episodeText.trim()) return // Don't run if script is empty

    // Debounce slightly to avoid rapid triggers on selection change
    const timer = setTimeout(() => {
        if (selected.step === 1 && !rawOutlineText) {
            executeEpisode().catch(() => {})
        } else if (selected.step === 2 && !rawAssetsVisualDnaText) {
            executeEpisode().catch(() => {})
        }
    }, 500)
    
    return () => clearTimeout(timer)
  }, [selected, selectedEpisode, rawOutlineText, rawAssetsVisualDnaText, execBusy, execRun?.status, episodeText, executeEpisode])

  const deleteEpisode = useCallback(async (epId: number) => {
    if (!projectId) return
    if (!window.confirm('确定删除此集？此操作不可恢复。')) return
    setBusy('create_episode') // Reuse busy state
    setError(null)
    try {
      await api.deleteEpisode(epId)
      if (selected.kind === 'episode' && selected.episodeId === epId) {
        setSelected({ kind: 'none' })
      }
      await refreshScript(projectId)
    } catch (e: any) {
      setError(extractErrorMessage(e, '删除集失败'))
    } finally {
      setBusy(null)
    }
  }, [projectId, selected, refreshScript])

  const goToStep1AndRun = useCallback(async () => {
      if (selected.kind !== 'episode') return
      // 1. Save
      try {
          await saveEpisode()
      } catch {
          return // Stop if save fails
      }
      // 2. Navigate
      setSelected({ ...selected, step: 1 })
      // 3. Execution is handled by useEffect auto-run logic when switching to step 1
      setRawOutlineText('') 
  }, [selected, saveEpisode])

  const goToStep2AndRun = useCallback(async () => {
    if (selected.kind !== 'episode') return
    setSelected({ ...selected, step: 2 })
    setRawAssetsVisualDnaText('')
  }, [selected])

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
        <ScriptSidebar
          episodes={episodes}
          selected={selected}
          onSelect={(episodeId, step) => setSelected({ kind: 'episode', episodeId, step })}
          onDeleteEpisode={deleteEpisode}
        />

        <div style={styles.main}>
          {selected.kind === 'none' ? <div style={styles.empty}>请从左侧选择一个 Step</div> : null}

          {selected.kind === 'episode' && selectedEpisode ? (
            <>
              {selected.step === 0 && (
                 <Step0_Script 
                   text={episodeText}
                   onChange={setEpisodeText}
                   onNext={goToStep1AndRun}
                   busy={busy === 'save_episode'}
                 />
               )}
               {selected.step === 1 && (
                 <Step1_Structure
                   rawText={rawOutlineText} 
                   onNext={goToStep2AndRun}
                   busy={execBusy}
                 />
               )}
               {selected.step === 2 && (
                 <Step2_Assets
                   rawText={rawAssetsVisualDnaText}
                   busy={execBusy}
                 />
               )}
            </>
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
    gridTemplateColumns: '280px 1fr', // Sidebar width
    gap: 12,
    alignItems: 'start',
    height: 'calc(100vh - 80px)', // Full height minus header/topbar
  },
  main: {
    height: '100%',
    overflow: 'hidden', // Let children handle scrolling
  },
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
