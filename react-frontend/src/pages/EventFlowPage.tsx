import { useEffect, useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import api from '../api/client'
import type { EpisodeRead, EventRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

export function EventFlowPage() {
  const { projects, projectId, setProjectId } = useProjectSelection()
  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [events, setEvents] = useState<EventRead[]>([])
  const [searchParams, setSearchParams] = useSearchParams()
  const [loading, setLoading] = useState(true)
  const [showAddSceneModal, setShowAddSceneModal] = useState(false)
  const [tempSelectedSceneIds, setTempSelectedSceneIds] = useState<Set<number>>(new Set())

  const eventId = useMemo(() => {
    const raw = searchParams.get('id')
    const v = raw ? Number(raw) : NaN
    return Number.isFinite(v) ? v : null
  }, [searchParams])

  useEffect(() => {
    let alive = true
    ;(async () => {
      if (!projectId) return
      setLoading(true)
      const [scriptRes, eventsRes] = await Promise.all([api.getScript(projectId), api.getEvents(projectId)])
      if (!alive) return
      setEpisodes(scriptRes.data || [])
      setEvents(eventsRes.data || [])
      setLoading(false)
    })().catch(() => setLoading(false))
    return () => {
      alive = false
    }
  }, [projectId])

  const currentEvent = useMemo(() => {
    if (!eventId) return (events[0] as EventRead | undefined) || null
    return events.find((e) => e.id === eventId) || null
  }, [events, eventId])

  // 如果 URL 没有 id，但有默认 event，则同步写入 URL
  useEffect(() => {
    if (!currentEvent) return
    if (!eventId) setSearchParams({ id: String(currentEvent.id) })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentEvent?.id])

  const linkedSceneIds = useMemo(() => {
    if (!currentEvent) return []
    return (currentEvent.nodes || [])
      .filter((n) => n.target_type === 'scene')
      .map((n) => Number(n.target_id))
  }, [currentEvent])

  const sortedLinkedScenes = useMemo(() => {
    const out: { sceneId: number; epOrder: number; sceneSeq: number; title?: string | null }[] = []
    const set = new Set(linkedSceneIds)
    for (const ep of episodes) {
      const scenes = (ep.scenes || []).slice().sort((a, b) => (a.sequence_number || 0) - (b.sequence_number || 0))
      for (const sc of scenes) {
        if (set.has(sc.id)) {
          out.push({ sceneId: sc.id, epOrder: ep.order, sceneSeq: Number(sc.sequence_number || 0), title: sc.title })
        }
      }
    }
    return out
  }, [episodes, linkedSceneIds])

  async function refreshEvents() {
    if (!projectId) return
    const res = await api.getEvents(projectId)
    setEvents(res.data || [])
  }

  async function updateEventMeta(next: Partial<Pick<EventRead, 'name' | 'color'>>) {
    if (!currentEvent) return
    await api.updateEvent(currentEvent.id, {
      name: next.name ?? currentEvent.name,
      color: next.color ?? currentEvent.color,
    })
    await refreshEvents()
  }

  function isSceneLinked(sceneId: number) {
    return linkedSceneIds.includes(sceneId)
  }

  function toggleSelectScene(sceneId: number) {
    setTempSelectedSceneIds((prev) => {
      const next = new Set(prev)
      if (next.has(sceneId)) next.delete(sceneId)
      else next.add(sceneId)
      return next
    })
  }

  async function saveSelectedScenes() {
    if (!currentEvent) return
    const ids = Array.from(tempSelectedSceneIds)
    if (ids.length === 0) return
    await Promise.all(
      ids.map((sceneId) =>
        api.upsertEventNode(currentEvent.id, {
          target_type: 'scene',
          target_id: sceneId,
          description: '',
        }),
      ),
    )
    await refreshEvents()
    setTempSelectedSceneIds(new Set())
    setShowAddSceneModal(false)
  }

  return (
    <div style={styles.page}>
      <div style={styles.topbar}>
        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'flex-start', flexWrap: 'wrap' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
            <Link to="/events" style={{ color: 'rgba(229,231,235,0.65)', textDecoration: 'none' }}>
              ← Back
            </Link>
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
            {currentEvent ? (
              <>
                <div style={{ fontSize: 18, fontWeight: 900 }}>{currentEvent.name}</div>
                <div style={{ ...styles.pill, background: currentEvent.color }}>Event</div>
              </>
            ) : null}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <button onClick={() => setShowAddSceneModal(true)} style={styles.btnPrimary}>
              + 增加关联场
            </button>
          </div>
        </div>

        {currentEvent ? (
          <div style={{ display: 'flex', gap: 12, alignItems: 'center', marginTop: 10 }}>
            <input
              value={currentEvent.name}
              onChange={(e) => {
                const v = e.target.value
                setEvents((prev) => prev.map((x) => (x.id === currentEvent.id ? { ...x, name: v } : x)))
              }}
              onBlur={() => updateEventMeta({ name: currentEvent.name }).catch(() => {})}
              style={{ ...styles.input, fontWeight: 800, width: 360 }}
            />
            <input
              type="color"
              value={currentEvent.color}
              onChange={(e) => {
                const v = e.target.value
                setEvents((prev) => prev.map((x) => (x.id === currentEvent.id ? { ...x, color: v } : x)))
                updateEventMeta({ color: v }).catch(() => {})
              }}
              style={styles.color}
            />
          </div>
        ) : null}
      </div>

      <div style={styles.canvas}>
        {loading ? (
          <div style={styles.empty}>加载中…</div>
        ) : sortedLinkedScenes.length === 0 ? (
          <div style={styles.empty}>
            <div style={{ textAlign: 'center' }}>
              <div style={{ marginBottom: 8, opacity: 0.8 }}>暂无关联场</div>
              <button onClick={() => setShowAddSceneModal(true)} style={styles.btn}>
                点击添加
              </button>
            </div>
          </div>
        ) : (
          <div style={styles.flowRow}>
            {sortedLinkedScenes.map((s, idx) => (
              <div key={s.sceneId} style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
                {idx > 0 ? <div style={{ width: 90, height: 4, background: currentEvent?.color || '#6366f1', opacity: 0.35 }} /> : null}
                <div style={{ ...styles.circle, borderColor: currentEvent?.color || '#6366f1' }}>
                  <div style={{ fontSize: 10, opacity: 0.65, fontWeight: 800 }}>EP {s.epOrder}</div>
                  <div style={{ fontSize: 14, fontWeight: 900 }}>Scene {s.sceneSeq}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 选择场弹窗 */}
      {showAddSceneModal ? (
        <div style={styles.modalMask}>
          <div style={styles.modal}>
            <div style={styles.modalHeader}>
              <div style={{ fontWeight: 900 }}>选择关联场 (Select Scenes)</div>
              <button onClick={() => setShowAddSceneModal(false)} style={styles.iconBtn}>
                ×
              </button>
            </div>
            <div style={styles.modalBody}>
              {episodes.map((ep) => (
                <div key={ep.id} style={styles.epBox}>
                  <div style={{ fontSize: 12, fontWeight: 900, opacity: 0.7, marginBottom: 10 }}>
                    EP{ep.order} - {ep.title}
                  </div>
                  <div style={styles.sceneGrid}>
                    {(ep.scenes || []).map((scene) => {
                      const linked = isSceneLinked(scene.id)
                      const picked = tempSelectedSceneIds.has(scene.id)
                      return (
                        <button
                          key={scene.id}
                          disabled={linked}
                          onClick={() => toggleSelectScene(scene.id)}
                          style={{
                            ...styles.sceneBtn,
                            ...(linked ? styles.sceneBtnLinked : picked ? styles.sceneBtnPicked : styles.sceneBtnIdle),
                          }}
                        >
                          <div style={{ fontWeight: 900 }}>Scene {scene.sequence_number}</div>
                          <div style={{ fontSize: 11, opacity: 0.7, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            {scene.title}
                          </div>
                          {linked ? <div style={styles.linkedTag}>Linked</div> : null}
                        </button>
                      )
                    })}
                  </div>
                </div>
              ))}
            </div>
            <div style={styles.modalFooter}>
              <button onClick={() => setShowAddSceneModal(false)} style={styles.btn}>
                取消
              </button>
              <button onClick={() => saveSelectedScenes().catch(() => {})} disabled={tempSelectedSceneIds.size === 0} style={styles.btnPrimary}>
                确认添加 ({tempSelectedSceneIds.size})
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: { display: 'flex', flexDirection: 'column', gap: 12 },
  topbar: { background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 12, padding: 12 },
  pill: { padding: '2px 8px', borderRadius: 999, color: '#fff', fontSize: 12, fontWeight: 900 },
  canvas: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.02)',
    height: 'calc(100vh - 220px)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    overflow: 'auto',
  },
  empty: { opacity: 0.7, padding: 16 },
  flowRow: { display: 'flex', alignItems: 'center', padding: 40 },
  circle: {
    width: 90,
    height: 90,
    borderRadius: 999,
    border: '4px solid rgba(99,102,241,0.8)',
    background: 'rgba(255,255,255,0.03)',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    boxShadow: '0 10px 20px rgba(0,0,0,0.25)',
  },
  select: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '8px 10px', outline: 'none' },
  input: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '8px 10px', outline: 'none' },
  color: { width: 34, height: 34, borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'transparent' },
  btn: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.14)', background: 'rgba(255,255,255,0.04)', color: '#e5e7eb', padding: '8px 10px', cursor: 'pointer' },
  btnPrimary: { borderRadius: 10, border: '1px solid rgba(99,102,241,0.6)', background: 'rgba(99,102,241,0.35)', color: '#fff', padding: '8px 10px', cursor: 'pointer' },
  modalMask: { position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 50 },
  modal: {
    width: 720,
    maxHeight: '80vh',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: '#0b1220',
    overflow: 'hidden',
    display: 'flex',
    flexDirection: 'column',
  },
  modalHeader: { padding: 12, display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid rgba(255,255,255,0.08)', background: 'rgba(0,0,0,0.18)' },
  iconBtn: { border: 'none', background: 'transparent', color: '#e5e7eb', cursor: 'pointer', fontSize: 20 },
  modalBody: { padding: 12, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 12 },
  modalFooter: { padding: 12, display: 'flex', justifyContent: 'flex-end', gap: 10, borderTop: '1px solid rgba(255,255,255,0.08)', background: 'rgba(0,0,0,0.18)' },
  epBox: { border: '1px solid rgba(255,255,255,0.08)', borderRadius: 12, padding: 12 },
  sceneGrid: { display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 10 },
  sceneBtn: { position: 'relative', height: 72, borderRadius: 12, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(255,255,255,0.02)', color: '#e5e7eb', padding: 10, textAlign: 'left', cursor: 'pointer', overflow: 'hidden' },
  sceneBtnIdle: {},
  sceneBtnPicked: { border: '1px solid rgba(99,102,241,0.8)', background: 'rgba(99,102,241,0.12)' },
  sceneBtnLinked: { opacity: 0.55, cursor: 'not-allowed', border: '1px solid rgba(255,255,255,0.06)', background: 'rgba(255,255,255,0.01)' },
  linkedTag: { position: 'absolute', top: 8, right: 8, fontSize: 10, opacity: 0.6 },
}


