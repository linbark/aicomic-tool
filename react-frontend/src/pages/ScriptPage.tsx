import { useEffect, useMemo, useRef, useState } from 'react'
import api from '../api/client'
import type { EpisodeRead, ShotRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

type Selected =
  | { kind: 'episode'; episodeId: number }
  | { kind: 'scene'; episodeId: number; sceneId: number }
  | { kind: 'shot'; episodeId: number; sceneId: number; shotId: number }
  | { kind: 'none' }

type ChatRole = 'user' | 'assistant'
type ChatMsg = { id: string; role: ChatRole; content: string; ts: number; cards?: any[]; debug?: any }

type StepUi = { index: number; action_key: string; why?: string | null; status: 'pending' | 'running' | 'done'; ms?: number; output_preview?: string }
type ChatRunUi = { runId: string; status: 'queued' | 'running' | 'done' | 'error'; steps: StepUi[]; error?: string | null }

function _now() {
  return Date.now()
}

function _trim(text: string, limit: number) {
  const t = String(text || '')
  return t.length > limit ? t.slice(0, limit) + '…' : t
}

  function _lsGet(key: string): string | null {
    try {
      return window.localStorage.getItem(key)
    } catch {
      return null
    }
  }
  function _lsSet(key: string, value: string) {
    try {
      window.localStorage.setItem(key, value)
    } catch {
      // ignore
    }
  }

function _chatKey(projectId: string, episodeId: number) {
  return `aicomic.episode_chat.${projectId}.${episodeId}`
}

export function ScriptPage() {
  const { projects, projectId, setProjectId, refreshProjects } = useProjectSelection()
  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [selected, setSelected] = useState<Selected>({ kind: 'none' })

  // Editor states
  const [episodeText, setEpisodeText] = useState('')
  const [sceneText, setSceneText] = useState('')
  const [shotDraft, setShotDraft] = useState<Partial<ShotRead> | null>(null)

  const [busy, setBusy] = useState<null | 'load' | 'save_episode' | 'save_scene' | 'save_shot' | 'create_episode' | 'create_scene' | 'create_shot'>(null)
  const [error, setError] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<null | 'project' | 'episode'>(null)
  const [showCreateProject, setShowCreateProject] = useState(false)
  const [createProjectName, setCreateProjectName] = useState('')
  const [createProjectDesc, setCreateProjectDesc] = useState('')
  const [creatingProject, setCreatingProject] = useState(false)

  // Chat (Episode only)
  const [chatMsgs, setChatMsgs] = useState<ChatMsg[]>([])
  const [chatInput, setChatInput] = useState('')
  const [chatBusy, setChatBusy] = useState(false)
  const [chatError, setChatError] = useState<string | null>(null)
  const [chatDebug, setChatDebug] = useState(false)
  const [cardBusy, setCardBusy] = useState<Record<string, boolean>>({})
  const [chatRun, setChatRun] = useState<ChatRunUi | null>(null)

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

  async function refreshScript(pid = projectId) {
    if (!pid) return
    setBusy('load')
    setError(null)
    try {
      const res = await api.getScript(pid)
    setEpisodes(res.data || [])
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '加载失败')
    } finally {
      setBusy(null)
    }
  }

  useEffect(() => {
    if (projectId) refreshScript(projectId).catch(() => {})
    else setEpisodes([])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId])

  // Sync editors when selection changes
  useEffect(() => {
    if (selected.kind === 'episode') {
      const ep = episodes.find((e) => e.id === selected.episodeId)
      setEpisodeText(String(ep?.description || ''))
      setSceneText('')
    setShotDraft(null)
      // load chat
      if (projectId) {
        const raw = _lsGet(_chatKey(projectId, selected.episodeId))
        if (raw) {
          try {
            const parsed = JSON.parse(raw)
            if (Array.isArray(parsed)) setChatMsgs(parsed as any)
    } catch {
            setChatMsgs([])
          }
        } else {
          setChatMsgs([])
        }
      }
          return
        }
    if (selected.kind === 'scene') {
      setEpisodeText('')
      setChatMsgs([])
      const sc = selectedScene
      setSceneText(String(sc?.description || ''))
      setShotDraft(null)
      return
    }
    if (selected.kind === 'shot') {
      setEpisodeText('')
      setSceneText('')
      setChatMsgs([])
      setShotDraft(selectedShot ? { ...selectedShot } : null)
      return
    }
    setEpisodeText('')
    setSceneText('')
    setChatMsgs([])
    setShotDraft(null)
  }, [selected, episodes, projectId, selectedScene, selectedShot])

  useEffect(() => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    _lsSet(_chatKey(projectId, selected.episodeId), JSON.stringify(chatMsgs.slice(-80)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatMsgs])

  // Poll run stages to show live execution steps
  useEffect(() => {
    if (!projectId) return
    if (!chatRun?.runId) return
    let alive = true
    const pid = projectId
    const runId = chatRun.runId
    let finalConsumed = false

    async function tick() {
      if (!alive) return
      try {
        const stagesRes = await api.listRunStages(pid, runId)
        const stages = (stagesRes.data as any)?.stages || []
        const stageList: string[] = Array.isArray(stages) ? stages.map((s: any) => String(s)) : []
        const stageSet = new Set(stageList)

        // status
        if (stageSet.has('chat.status')) {
          const st = await api.getRunStage(pid, runId, 'chat.status')
          const sd = (st.data as any)?.data || {}
          const s = sd?.status
          const curIdx = typeof sd?.current_step_index === 'number' ? Number(sd.current_step_index) : null
          const curAk = sd?.current_action_key ? String(sd.current_action_key) : null
          if (s && alive) {
            setChatRun((prev) => {
              if (!prev) return prev
              const next: any = { ...prev, status: s }
              // 兜底：用 current_step_index 标记 running（即使 start stage 没被前端识别到）
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
              // 如果后端已经标记 done，则所有 steps 直接 done
              if (String(s) === 'done' && prev.steps.length) {
                next.steps = prev.steps.map((x) => ({ ...x, status: 'done' }))
              }
              return next
            })
          }
        }

        // plan
        if (stageSet.has('chat.plan')) {
          const pl = await api.getRunStage(pid, runId, 'chat.plan')
          const plan = (pl.data as any)?.data?.plan
          const stepsArr = Array.isArray(plan?.steps) ? plan.steps : []
          if (alive && stepsArr.length) {
            setChatRun((prev) => {
              if (!prev) return prev
              // initialize only once or when empty
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

        // step start/end
        const stepStartRe = /^chat\\.step\\.(\\d+)\\.start$/
        const stepEndRe = /^chat\\.step\\.(\\d+)\\.end$/
        for (const name of stageSet) {
          let m = String(name).match(stepStartRe)
          if (m) {
            const idx = Number(m[1])
            if (Number.isFinite(idx)) {
              setChatRun((prev) => {
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
            setChatRun((prev) => {
              if (!prev) return prev
              const steps = prev.steps.slice()
              const cur = steps[idx]
              if (cur) steps[idx] = { ...cur, status: 'done', ms: Number.isFinite(ms) ? ms : undefined, output_preview: outputPreview }
              return { ...prev, steps }
            })
          }
        }

        // final / error
        // step error（优先展示）
        for (const n of stageSet) {
          const name = String(n)
          const m = name.match(/^chat\.step\.(\d+)\.error$/)
          if (!m) continue
          const idx = Number(m[1])
          if (!Number.isFinite(idx)) continue
          const erd = await api.getRunStage(pid, runId, name)
          const msg = (erd.data as any)?.data?.error
          if (alive) {
            setChatRun((prev) => (prev ? { ...prev, status: 'error', error: String(msg || 'step_error') } : prev))
            setChatBusy(false)
          }
        }
        if (stageSet.has('chat.error')) {
          const er = await api.getRunStage(pid, runId, 'chat.error')
          const msg = (er.data as any)?.data?.error
          if (alive) setChatRun((prev) => (prev ? { ...prev, status: 'error', error: String(msg || 'error') } : prev))
          if (alive) setChatBusy(false)
        }
        if (stageSet.has('chat.final')) {
          if (!finalConsumed) {
            finalConsumed = true
            const fin = await api.getRunStage(pid, runId, 'chat.final')
            const d = (fin.data as any)?.data || {}
            const assistantText = String(d?.assistant_message || '完成')
            const cards = Array.isArray(d?.cards) ? d.cards : undefined
            if (alive) {
              setChatMsgs((prev) => [...prev, { id: `${_now()}-a`, role: 'assistant', content: assistantText, ts: _now(), cards }])
              setChatBusy(false)
              setChatRun((prev) => {
                if (!prev) return prev
                return { ...prev, status: 'done', steps: prev.steps.map((x) => ({ ...x, status: 'done' })) }
              })
            }
          }
        }
    } catch (e: any) {
        // ignore transient polling errors
      }
    }

    const timer = window.setInterval(() => tick().catch(() => {}), 800)
    tick().catch(() => {})
    return () => {
      alive = false
      window.clearInterval(timer)
    }
  }, [projectId, chatRun?.runId])

  async function saveEpisode() {
    if (!projectId || selected.kind !== 'episode') return
    setBusy('save_episode')
    setError(null)
    try {
      episodeDirtyForIdRef.current = selected.episodeId
      await api.updateEpisode(selected.episodeId, { description: episodeText })
      await refreshScript(projectId)
      setSelected({ kind: 'episode', episodeId: selected.episodeId })
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setBusy(null)
    }
  }

  async function saveScene() {
    if (!projectId || selected.kind !== 'scene' || !selectedScene) return
    setBusy('save_scene')
    setError(null)
    try {
      sceneDirtyForIdRef.current = selectedScene.id
      await api.updateScene(selectedScene.id, { description: sceneText })
      await refreshScript(projectId)
      setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selectedScene.id })
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setBusy(null)
    }
  }

  async function saveShot() {
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
      setError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setBusy(null)
    }
  }

  async function createEpisode() {
    if (!projectId) return
    setBusy('create_episode')
    setError(null)
    try {
      await api.createEpisode(projectId, { title: `第${episodes.length + 1}集` })
      await refreshScript(projectId)
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '新建集失败')
    } finally {
      setBusy(null)
    }
  }

  async function createProject() {
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
      const pid = Number((res.data as any)?.id)
      await refreshProjects()
      if (Number.isFinite(pid)) setProjectId(pid)
      setShowCreateProject(false)
      setCreateProjectName('')
      setCreateProjectDesc('')
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '新建项目失败')
    } finally {
      setCreatingProject(false)
    }
  }

  async function deleteCurrentProject() {
    if (!projectId) return
    if (!window.confirm('确定删除当前项目？此操作不可恢复。')) return
    setDeleting('project')
    setError(null)
    try {
      await api.deleteProject(projectId)
      // 重新加载项目列表 & 清空选择
      setProjectId(null)
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '删除项目失败')
    } finally {
      setDeleting(null)
    }
  }

  async function deleteCurrentEpisode() {
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
      setError(e?.response?.data?.detail || e?.message || '删除集失败')
    } finally {
      setDeleting(null)
    }
  }

  async function createScene() {
    if (!projectId || selected.kind !== 'episode') return
    setBusy('create_scene')
    setError(null)
    try {
      await api.createScene(selected.episodeId, { title: `场${(selectedEpisode?.scenes || []).length + 1}` })
      await refreshScript(projectId)
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '新建场失败')
    } finally {
      setBusy(null)
    }
  }

  async function createShot() {
    if (!projectId || selected.kind !== 'scene' || !selectedScene) return
    setBusy('create_shot')
    setError(null)
    try {
      await api.createShot(selectedScene.id, { title: `镜头${(selectedScene.shots || []).length + 1}`, action_text: '' })
      await refreshScript(projectId)
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '新建镜头失败')
    } finally {
      setBusy(null)
    }
  }

  async function sendChat() {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const msg = (chatInput || '').trim()
    if (!msg) return

    const userMsg: ChatMsg = { id: `${_now()}-u`, role: 'user', content: msg, ts: _now() }
    setChatMsgs((prev) => [...prev, userMsg])
    setChatInput('')
    setChatError(null)
    setChatBusy(true)
    setChatRun(null)
    try {
      const res = await api.aiChatActAsync({
        project_id: projectId,
        episode_id: selected.episodeId,
        current_action_key: 'episode_chat',
        message: msg,
        debug: chatDebug,
        ui_context: {
          master_script: episodeText,
          episode_meta: {
            id: selectedEpisode?.id,
            title: selectedEpisode?.title,
            order: selectedEpisode?.order,
          },
        },
      } as any)
      const data = res.data as any
      const runId = String(data?.run_id || '')
      if (runId) {
        setChatRun({ runId, status: 'queued', steps: [] })
        setChatMsgs((prev) => [...prev, { id: `${_now()}-a`, role: 'assistant', content: `已开始执行（run=${runId}）…`, ts: _now() }])
      } else {
        setChatBusy(false)
        setChatMsgs((prev) => [...prev, { id: `${_now()}-a`, role: 'assistant', content: '启动失败：缺少 run_id', ts: _now() }])
      }
    } catch (e: any) {
      setChatError(e?.response?.data?.detail || e?.message || '执行失败')
    } finally {
      // busy 在轮询完成时关闭
    }
  }

  async function handleCardApproveChangeSet(changesetId: string) {
    if (!changesetId) return
    setCardBusy((prev) => ({ ...prev, [`approve:${changesetId}`]: true }))
    try {
      await api.memoryApproveChangeSet(changesetId, { reviewer: 'human', note: null })
      setChatMsgs((prev) => [
        ...prev,
        { id: `${_now()}-a`, role: 'assistant', content: `已确认提交：${changesetId}（已落库 + materialize）`, ts: _now() },
      ])
    } catch (e: any) {
      setChatMsgs((prev) => [
        ...prev,
        { id: `${_now()}-a`, role: 'assistant', content: `提交失败：${changesetId}\n${e?.response?.data?.detail || e?.message || '未知错误'}`, ts: _now() },
      ])
    } finally {
      setCardBusy((prev) => ({ ...prev, [`approve:${changesetId}`]: false }))
    }
  }

  async function handleCardRejectChangeSet(changesetId: string) {
    if (!changesetId) return
    setCardBusy((prev) => ({ ...prev, [`reject:${changesetId}`]: true }))
    try {
      await api.memoryRejectChangeSet(changesetId, { reviewer: 'human', note: 'rejected_in_chat' })
      setChatMsgs((prev) => [...prev, { id: `${_now()}-a`, role: 'assistant', content: `已驳回：${changesetId}`, ts: _now() }])
    } catch (e: any) {
      setChatMsgs((prev) => [
        ...prev,
        { id: `${_now()}-a`, role: 'assistant', content: `驳回失败：${changesetId}\n${e?.response?.data?.detail || e?.message || '未知错误'}`, ts: _now() },
      ])
    } finally {
      setCardBusy((prev) => ({ ...prev, [`reject:${changesetId}`]: false }))
    }
  }

  function handleCardChooseIntent(label: string) {
    const t = (label || '').trim()
    if (!t) return
    setChatInput(`我的意图：${t}`)
  }

  function renderCards(cards: any[] | undefined) {
    if (!cards || !cards.length) return null
  return (
      <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 8 }}>
        {cards.map((c, idx) => (
          <div key={idx} style={{ border: '1px solid rgba(255,255,255,0.1)', borderRadius: 10, padding: 10, background: 'rgba(0,0,0,0.12)' }}>
            <div style={{ fontWeight: 700, fontSize: 12 }}>{String(c?.title || c?.type || 'Card')}</div>
            {c?.summary ? <div style={{ marginTop: 6, fontSize: 12, opacity: 0.85, whiteSpace: 'pre-wrap' }}>{String(c.summary)}</div> : null}
            {c?.hint ? <div style={{ marginTop: 6, fontSize: 11, opacity: 0.65, whiteSpace: 'pre-wrap' }}>{String(c.hint)}</div> : null}

            {/* 澄清意图卡片 */}
            {c?.type === 'clarify_intent' && Array.isArray(c?.options) ? (
              <div style={{ marginTop: 8, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                {c.options.slice(0, 10).map((opt: any, j: number) => (
                  <button key={j} style={styles.btn} onClick={() => handleCardChooseIntent(String(opt?.label || opt?.value || ''))}>
                    {String(opt?.label || opt?.value || '选项')}
                  </button>
                ))}
              </div>
            ) : null}

            {/* 审阅 ChangeSet 卡片 */}
            {c?.type === 'review_changeset' && c?.changeset_id ? (
              <div style={{ marginTop: 8, display: 'flex', gap: 8 }}>
          <button
                  style={styles.btnPrimary}
                  disabled={!!cardBusy[`approve:${String(c.changeset_id)}`]}
                  onClick={() => handleCardApproveChangeSet(String(c.changeset_id)).catch(() => {})}
                >
                  {cardBusy[`approve:${String(c.changeset_id)}`] ? '提交中…' : '确认提交'}
                </button>
                <button
                  style={styles.btn}
                  disabled={!!cardBusy[`reject:${String(c.changeset_id)}`]}
                  onClick={() => handleCardRejectChangeSet(String(c.changeset_id)).catch(() => {})}
                >
                  {cardBusy[`reject:${String(c.changeset_id)}`] ? '驳回中…' : '驳回'}
          </button>
        </div>
            ) : null}
          </div>
        ))}
      </div>
    )
  }

  return (
    <div style={styles.page}>
      <div style={styles.topbar}>
        <div style={{ fontWeight: 700 }}>剧本</div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <select value={projectId ?? ''} onChange={(e) => setProjectId(e.target.value ? Number(e.target.value) : null)} style={styles.select}>
            <option value="" disabled>
              选择项目…
            </option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <button style={styles.btn} onClick={() => projectId && refreshScript(projectId).catch(() => {})} disabled={!projectId || busy === 'load'}>
            {busy === 'load' ? '加载中…' : '刷新'}
          </button>
          <button style={styles.btnPrimary} onClick={() => setShowCreateProject(true)}>
            + 新建项目
          </button>
          <button style={styles.btnPrimary} onClick={() => createEpisode().catch(() => {})} disabled={!projectId || busy === 'create_episode'}>
            + 新建集
          </button>
          <button style={styles.btn} onClick={() => deleteCurrentProject().catch(() => {})} disabled={!projectId || deleting === 'project'}>
            {deleting === 'project' ? '删除中…' : '删除项目'}
          </button>
        </div>
      </div>

      {showCreateProject ? (
        <div style={{ ...styles.panel, marginBottom: 12 }}>
          <div style={{ ...styles.panelHeader, marginBottom: 12 }}>
            <div style={{ fontWeight: 700 }}>新建项目</div>
            <div style={{ display: 'flex', gap: 8 }}>
              <button style={styles.btn} onClick={() => setShowCreateProject(false)} disabled={creatingProject}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => createProject().catch(() => {})} disabled={creatingProject || !createProjectName.trim()}>
                {creatingProject ? '创建中…' : '创建'}
              </button>
            </div>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
            <div>
              <div style={styles.labelRow}>
                <div style={styles.label}>项目名称</div>
              </div>
              <input value={createProjectName} onChange={(e) => setCreateProjectName(e.target.value)} style={styles.select} placeholder="例如：西游记" />
              </div>
            <div>
              <div style={styles.labelRow}>
                <div style={styles.label}>项目简介（可选）</div>
            </div>
              <input value={createProjectDesc} onChange={(e) => setCreateProjectDesc(e.target.value)} style={styles.select} placeholder="一句话描述故事/风格…" />
            </div>
          </div>
        </div>
      ) : null}

      {error ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 10 }}>{error}</div> : null}

      <div style={styles.body}>
        <div style={styles.sidebar}>
          <div style={styles.sidebarSection}>
            <div style={styles.colHeader}>
              <div style={styles.colTitle}>Episodes</div>
            </div>
            <div style={styles.colScroll} className="aicomic-scroll">
              {episodes.map((ep) => (
                <div key={ep.id} style={styles.card}>
                  <button
                  style={{
                      ...styles.cardBtn,
                    ...(selected.kind !== 'none' && selectedEpisode?.id === ep.id ? styles.cardActive : null),
                  }}
                  onClick={() => setSelected({ kind: 'episode', episodeId: ep.id })}
                >
                      <div style={styles.cardTitle}>
                        <span style={styles.mono}>EP{ep.order}</span> {ep.title}
                      </div>
                    {ep.description ? <div style={styles.cardSub}>{_trim(ep.description, 80)}</div> : null}
                    </button>

                  {(ep.scenes || []).length ? (
                    <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 6, paddingLeft: 10 }}>
                      {(ep.scenes || []).map((sc) => (
                        <button
                      key={sc.id}
                      style={{
                            ...styles.smallListBtn,
                            ...(selected.kind !== 'none' && selectedScene?.id === sc.id ? styles.smallListBtnActive : null),
                          }}
                          onClick={() => setSelected({ kind: 'scene', episodeId: ep.id, sceneId: sc.id })}
                        >
                          <span style={styles.mono}>SC{sc.sequence_number ?? ''}</span> {sc.title || '未命名场景'}
                    </button>
                      ))}
                    </div>
              ) : null}
                    </div>
                  ))}
            </div>
          </div>
        </div>

        <div style={styles.main}>
          {selected.kind === 'none' ? <div style={styles.empty}>请选择一个 Episode 或 Scene</div> : null}

            {selected.kind === 'episode' && selectedEpisode ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    <span style={styles.mono}>EP{selectedEpisode.order}</span> {selectedEpisode.title}
                  </div>
                <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                  <button style={styles.btn} onClick={() => createScene().catch(() => {})} disabled={busy === 'create_scene'}>
                    + 新建场
                    </button>
                  <button style={styles.btnPrimary} onClick={() => saveEpisode().catch(() => {})} disabled={busy === 'save_episode'}>
                    保存本集
                    </button>
                  <button style={styles.btn} onClick={() => deleteCurrentEpisode().catch(() => {})} disabled={deleting === 'episode'}>
                    {deleting === 'episode' ? '删除中…' : '删除本集'}
                    </button>
                  </div>
                </div>

              <div style={{ marginBottom: 10, fontSize: 12, opacity: 0.7 }}>
                Episode-详情已改为 Chat 驱动：请直接对话提出意图（生成/优化/分镜/入库审阅），系统会自动规划执行路径。
                </div>

                    <div style={styles.labelRow}>
                <div style={styles.label}>当前剧本（可选：直接编辑）</div>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, opacity: 0.75 }}>
                  <input type="checkbox" checked={chatDebug} onChange={(e) => setChatDebug(e.target.checked)} />
                  Debug
                </label>
                    </div>
              <textarea value={episodeText} onChange={(e) => setEpisodeText(e.target.value)} style={{ ...styles.textarea, height: 220 }} placeholder="本集剧本内容…" />

              <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', margin: '12px 0' }} />

              <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 8 }}>Chat（唯一入口）</div>
              {chatError ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }}>{chatError}</div> : null}

              {/* 执行步骤（执行中可见） */}
              {chatRun ? (
                <div style={{ marginBottom: 10, border: '1px solid rgba(255,255,255,0.08)', borderRadius: 10, padding: 10, background: 'rgba(0,0,0,0.10)' }}>
                                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85 }}>执行步骤</div>
                    <div style={{ fontSize: 11, opacity: 0.65 }}>
                      状态：{chatRun.status} {chatRun.runId ? `（${chatRun.runId}）` : ''}
                                      </div>
                                    </div>
                  {chatRun.error ? <div style={{ marginTop: 6, color: '#f87171', fontSize: 12 }}>{chatRun.error}</div> : null}
                  {chatRun.steps.length ? (
                    <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
                      {chatRun.steps.map((s) => (
                        <div key={s.index} style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
                          <div style={{ width: 22, opacity: 0.6, fontFamily: 'monospace' }}>{s.index + 1}</div>
                          <div style={{ flex: 1 }}>
                            <div style={{ fontSize: 12, opacity: 0.92 }}>
                              {s.action_key} <span style={{ opacity: 0.65 }}>[{s.status}{typeof s.ms === 'number' ? ` ${s.ms}ms` : ''}]</span>
                                  </div>
                            {s.why ? <div style={{ fontSize: 11, opacity: 0.65, whiteSpace: 'pre-wrap' }}>{s.why}</div> : null}
                            {s.output_preview ? (
                              <div style={{ marginTop: 4, fontSize: 11, opacity: 0.7, whiteSpace: 'pre-wrap' }}>{_trim(s.output_preview, 500)}</div>
                    ) : null}
                  </div>
                              </div>
                            ))}
                          </div>
                  ) : (
                    <div style={{ marginTop: 8, fontSize: 12, opacity: 0.6 }}>规划中…</div>
                        )}
                      </div>
              ) : null}

                      <div
                        className="aicomic-scroll"
                        style={{
                  maxHeight: 240,
                          overflowY: 'auto',
                          background: 'rgba(0,0,0,0.12)',
                          border: '1px solid rgba(255,255,255,0.06)',
                          borderRadius: 10,
                          padding: 10,
                          marginBottom: 10,
                        }}
                      >
                        {chatMsgs.length === 0 ? (
                  <div style={{ fontSize: 12, opacity: 0.6 }}>
                    例：帮我提取本集大纲并优化节奏，减少对白；完成后把变更提交给我确认。
                  </div>
                ) : (
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                    {chatMsgs.slice(-40).map((m) => (
                      <div key={m.id}>
                        <div style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
                                <div style={{ width: 48, fontSize: 11, opacity: 0.6 }}>{m.role === 'user' ? '用户' : '系统'}</div>
                          <div style={{ flex: 1, whiteSpace: 'pre-wrap', fontSize: 12, lineHeight: 1.5, opacity: 0.92 }}>{m.content}</div>
                                </div>
                        {renderCards(m.cards)}
                        {chatDebug && m.debug ? (
                          <details style={{ marginTop: 8 }}>
                            <summary style={{ cursor: 'pointer', fontSize: 11, opacity: 0.7 }}>debug</summary>
                            <pre style={{ fontSize: 11, opacity: 0.85, whiteSpace: 'pre-wrap' }}>{JSON.stringify(m.debug, null, 2)}</pre>
                          </details>
                        ) : null}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>

                      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                        <input
                          value={chatInput}
                          onChange={(e) => setChatInput(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) sendChat().catch(() => {})
                          }}
                          placeholder="输入你的意图（Ctrl/Cmd + Enter 发送）"
                          style={{ ...styles.select, flex: 1, padding: '10px 12px' }}
                  disabled={!projectId || chatBusy}
                        />
                        <button style={styles.btnPrimary} onClick={() => sendChat().catch(() => {})} disabled={chatBusy || !chatInput.trim()}>
                          {chatBusy ? '执行中…' : '发送'}
                        </button>
                      </div>
              </section>
            ) : null}

            {selected.kind === 'scene' && selectedScene ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                <div style={{ fontWeight: 700 }}>
                  <span style={styles.mono}>SC{selectedScene.sequence_number ?? ''}</span> {selectedScene.title || '未命名场景'}
                  </div>
                <div style={{ display: 'flex', gap: 8 }}>
                  <button style={styles.btn} onClick={() => createShot().catch(() => {})} disabled={busy === 'create_shot'}>
                    + 新建镜头
                  </button>
                  <button style={styles.btnPrimary} onClick={() => saveScene().catch(() => {})} disabled={busy === 'save_scene'}>
                    保存本场
                    </button>
                  </div>
                </div>

                  <div style={styles.labelRow}>
                <div style={styles.label}>场景内容</div>
                <div style={{ fontSize: 12, opacity: 0.5 }}>AI 功能已下线；请在 Episode Chat 中驱动工作流</div>
              </div>
              <textarea value={sceneText} onChange={(e) => setSceneText(e.target.value)} style={{ ...styles.textarea, height: 220 }} />

              <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', margin: '12px 0' }} />
              <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 8 }}>镜头列表</div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                {(selectedScene.shots || []).map((sh) => (
                    <button
                    key={sh.id}
                    style={{ ...styles.smallListBtn, ...(selectedShot?.id === sh.id ? styles.smallListBtnActive : null) }}
                    onClick={() => setSelected({ kind: 'shot', episodeId: selected.episodeId, sceneId: selectedScene.id, shotId: sh.id })}
                  >
                    <span style={styles.mono}>SH{sh.sequence_number ?? ''}</span> {sh.title || '未命名镜头'}
                    </button>
                ))}
                {!(selectedScene.shots || []).length ? <div style={{ fontSize: 12, opacity: 0.6 }}>暂无镜头</div> : null}
                </div>
              </section>
            ) : null}

          {selected.kind === 'shot' && selectedShot && shotDraft ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700 }}>
                  <span style={styles.mono}>SH{selectedShot.sequence_number ?? ''}</span> {selectedShot.title || '未命名镜头'}
                  </div>
                <button style={styles.btnPrimary} onClick={() => saveShot().catch(() => {})} disabled={busy === 'save_shot'}>
                    保存镜头
                  </button>
                </div>

              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                  <div>
                  <div style={styles.labelRow}>
                    <div style={styles.label}>标题</div>
                  </div>
                  <input value={String(shotDraft.title || '')} onChange={(e) => setShotDraft((p) => ({ ...(p || {}), title: e.target.value }))} style={styles.select} />
                  </div>
                  <div>
                  <div style={styles.labelRow}>
                    <div style={styles.label}>对白（可选）</div>
                  </div>
                  <input value={String(shotDraft.dialogue || '')} onChange={(e) => setShotDraft((p) => ({ ...(p || {}), dialogue: e.target.value }))} style={styles.select} />
                  </div>
                </div>

                <div style={{ marginTop: 10 }}>
                <div style={styles.labelRow}>
                  <div style={styles.label}>动作</div>
                </div>
                <textarea value={String(shotDraft.action_text || '')} onChange={(e) => setShotDraft((p) => ({ ...(p || {}), action_text: e.target.value }))} style={{ ...styles.textarea, height: 160 }} />
                </div>
              </section>
            ) : null}
          </div>
        </div>
                            </div>
                          )
                        }

const styles: Record<string, any> = {
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
  sidebar: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.04)',
    overflow: 'hidden',
  },
  sidebarSection: {
    display: 'flex',
    flexDirection: 'column',
    height: 'calc(100vh - 110px)',
  },
  colHeader: {
    padding: 12,
    borderBottom: '1px solid rgba(255,255,255,0.08)',
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  colTitle: { fontWeight: 700, fontSize: 13 },
  colScroll: { padding: 10, overflowY: 'auto', flex: 1 },
  main: {},
  panel: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.04)',
    padding: 12,
  },
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 10,
    marginBottom: 10,
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
  },
  textarea: {
    width: '100%',
    background: 'rgba(0,0,0,0.22)',
    border: '1px solid rgba(255,255,255,0.12)',
    color: 'rgba(255,255,255,0.92)',
    borderRadius: 10,
    padding: 10,
    outline: 'none',
    resize: 'vertical',
    fontSize: 12,
    lineHeight: 1.5,
  },
  btn: {
    background: 'rgba(255,255,255,0.04)',
    border: '1px solid rgba(255,255,255,0.14)',
    color: 'rgba(255,255,255,0.92)',
    borderRadius: 10,
    padding: '8px 10px',
    cursor: 'pointer',
  },
  btnPrimary: {
    background: 'rgba(99,102,241,0.35)',
    border: '1px solid rgba(99,102,241,0.6)',
    color: 'rgba(255,255,255,0.95)',
    borderRadius: 10,
    padding: '8px 10px',
    cursor: 'pointer',
  },
  card: {
    marginBottom: 10,
  },
  cardBtn: {
    width: '100%',
    textAlign: 'left',
    background: 'rgba(0,0,0,0.18)',
    border: '1px solid rgba(255,255,255,0.08)',
    color: 'rgba(255,255,255,0.92)',
    borderRadius: 10,
    padding: 10,
    cursor: 'pointer',
  },
  cardActive: {
    border: '1px solid rgba(99,102,241,0.7)',
    background: 'rgba(99,102,241,0.12)',
  },
  cardTitle: { fontWeight: 700, fontSize: 12, marginBottom: 4 },
  cardSub: { fontSize: 11, opacity: 0.7, whiteSpace: 'pre-wrap' as const },
  labelRow: { display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 },
  label: { fontSize: 12, fontWeight: 700, opacity: 0.85 },
  smallListBtn: {
    width: '100%',
    textAlign: 'left',
    background: 'rgba(0,0,0,0.16)',
    border: '1px solid rgba(255,255,255,0.06)',
    color: 'rgba(255,255,255,0.9)',
    borderRadius: 10,
    padding: '8px 10px',
    cursor: 'pointer',
    fontSize: 12,
  },
  smallListBtnActive: {
    border: '1px solid rgba(99,102,241,0.6)',
    background: 'rgba(99,102,241,0.10)',
  },
  mono: { fontFamily: 'monospace', fontSize: 11, opacity: 0.85 },
}


