import { useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import api from '../api/client'
import type { EpisodeRead, EventRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

type LinePath = { id: string; d: string; color: string }

export function EventMatrixPage() {
  const { projects, projectId, setProjectId } = useProjectSelection()
  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [events, setEvents] = useState<EventRead[]>([])
  const [selectedEventIds, setSelectedEventIds] = useState<number[]>([])
  const [error, setError] = useState<string>('')

  // 搜索 / 排序
  const [eventQuery, setEventQuery] = useState('')
  const [eventSort, setEventSort] = useState<'name_asc' | 'name_desc' | 'linked_desc' | 'linked_asc'>('name_asc')

  const [showCreate, setShowCreate] = useState(false)
  const [createForm, setCreateForm] = useState<{ name: string; color: string; description: string }>({
    name: '',
    color: '#3B82F6',
    description: '',
  })

  const [isSaving, setIsSaving] = useState(false)
  const [isDeleting, setIsDeleting] = useState(false)

  // 事件列表内联编辑（无需单选）
  const [inlineEditEventId, setInlineEditEventId] = useState<number | null>(null)
  const [inlineEditDraft, setInlineEditDraft] = useState<{ name: string; color: string; description: string }>({
    name: '',
    color: '#3B82F6',
    description: '',
  })
  const [inlineSaving, setInlineSaving] = useState(false)

  const [scale, setScale] = useState(1)
  const gridContainerRef = useRef<HTMLDivElement | null>(null)
  const contentContainerRef = useRef<HTMLDivElement | null>(null)
  const sceneRefs = useRef(new Map<number, HTMLDivElement>())
  const [linePaths, setLinePaths] = useState<LinePath[]>([])

  // drag-to-pan
  const dragging = useRef(false)
  const dragStart = useRef({ x: 0, y: 0, sl: 0, st: 0 })

  useEffect(() => {
    let alive = true
    ;(async () => {
      if (!projectId) return
      setError('')
      const [scriptRes, eventsRes] = await Promise.all([api.getScript(projectId), api.getEvents(projectId)])
      if (!alive) return
      setEpisodes(scriptRes.data || [])
      setEvents(eventsRes.data || [])
      setSelectedEventIds((prev) => (prev.length ? prev : (eventsRes.data || []).slice(0, 1).map((e) => e.id)))
    })().catch(() => {
      setError('加载失败：请确认后端已启动且可访问')
    })
    return () => {
      alive = false
    }
  }, [projectId])

  const episodeRows = useMemo(() => {
    return (episodes || []).map((ep) => ({
      ...ep,
      scenes: (ep.scenes || []).slice().sort((a, b) => (a.sequence_number || 0) - (b.sequence_number || 0)),
    }))
  }, [episodes])

  const maxSceneCount = useMemo(() => {
    if (!episodeRows.length) return 5
    return Math.max(...episodeRows.map((ep) => (ep.scenes?.length || 0)), 5)
  }, [episodeRows])

  const sceneEventMap = useMemo(() => {
    const map = new Map<number, EventRead[]>()
    for (const evt of events || []) {
      for (const node of evt.nodes || []) {
        if (node.target_type !== 'scene') continue
        const sceneId = Number(node.target_id)
        if (!map.has(sceneId)) map.set(sceneId, [])
        map.get(sceneId)!.push(evt)
      }
    }
    return map
  }, [events])

  const singleEventSelected = selectedEventIds.length === 1
  const editLink = singleEventSelected ? `/events/flow?id=${selectedEventIds[0]}` : ''

  const selectedEvent = useMemo(() => {
    if (!singleEventSelected) return null
    return events.find((e) => e.id === selectedEventIds[0]) || null
  }, [events, selectedEventIds, singleEventSelected])

  const [editDraft, setEditDraft] = useState<{ name: string; color: string; description: string }>({
    name: '',
    color: '#3B82F6',
    description: '',
  })

  useEffect(() => {
    if (!selectedEvent) return
    setEditDraft({
      name: selectedEvent.name || '',
      color: selectedEvent.color || '#3B82F6',
      description: selectedEvent.description || '',
    })
  }, [selectedEvent?.id])

  useEffect(() => {
    if (inlineEditEventId == null) return
    const evt = events.find((e) => e.id === inlineEditEventId)
    if (!evt) return
    setInlineEditDraft({
      name: evt.name || '',
      color: evt.color || '#3B82F6',
      description: evt.description || '',
    })
  }, [inlineEditEventId, events])

  function isEventSelected(id: number) {
    return selectedEventIds.includes(id)
  }

  const filteredSortedEvents = useMemo(() => {
    const q = (eventQuery || '').trim().toLowerCase()
    const withMeta = (events || []).map((e) => {
      const linked = (e.nodes || []).filter((n) => n.target_type === 'scene').length
      return { e, linked }
    })

    const filtered = q
      ? withMeta.filter(({ e }) => {
          const name = (e.name || '').toLowerCase()
          const desc = (e.description || '').toLowerCase()
          return name.includes(q) || desc.includes(q)
        })
      : withMeta

    const sorted = filtered.slice().sort((a, b) => {
      if (eventSort === 'linked_desc') return b.linked - a.linked || a.e.name.localeCompare(b.e.name)
      if (eventSort === 'linked_asc') return a.linked - b.linked || a.e.name.localeCompare(b.e.name)
      if (eventSort === 'name_desc') return b.e.name.localeCompare(a.e.name)
      return a.e.name.localeCompare(b.e.name)
    })

    return sorted.map((x) => x.e)
  }, [events, eventQuery, eventSort])

  function isSceneActive(sceneId: number) {
    const evts = sceneEventMap.get(sceneId) || []
    return evts.some((e) => selectedEventIds.includes(e.id))
  }

  function getSceneStyle(sceneId: number): React.CSSProperties {
    const evts = sceneEventMap.get(sceneId) || []
    const active = evts.filter((e) => selectedEventIds.includes(e.id))
    if (active.length === 0) return {}
    if (active.length === 1) return { backgroundColor: active[0].color, borderColor: active[0].color }
    const seg = 360 / active.length
    const conic = `conic-gradient(${active
      .map((e, i) => `${e.color} ${i * seg}deg ${(i + 1) * seg}deg`)
      .join(',')})`
    return { background: conic, borderColor: 'rgba(255,255,255,0.15)' }
  }

  function setSceneRef(sceneId: number, el: HTMLDivElement | null) {
    if (el) sceneRefs.current.set(sceneId, el)
    else sceneRefs.current.delete(sceneId)
  }

  function selectAll() {
    // “全选”以当前筛选结果为准，避免搜索时把隐藏事件也选上
    setSelectedEventIds(filteredSortedEvents.map((e) => e.id))
  }
  function clearSelection() {
    setSelectedEventIds([])
  }

  async function refreshEvents(keepSelection = true) {
    if (!projectId) return
    const res = await api.getEvents(projectId)
    setEvents(res.data || [])
    if (!keepSelection) setSelectedEventIds([])
  }

  async function handleCreate() {
    if (!projectId) return
    const name = createForm.name.trim()
    if (!name) return
    setError('')
    try {
      const res = await api.createEvent(projectId, {
        name,
        color: createForm.color || '#3B82F6',
        description: createForm.description || '',
      })
      setShowCreate(false)
      setCreateForm({ name: '', color: '#3B82F6', description: '' })
      await refreshEvents(false)
      setSelectedEventIds([res.data.id])
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '新建失败')
    }
  }

  async function handleSaveSelected() {
    if (!selectedEvent) return
    setIsSaving(true)
    setError('')
    try {
      await api.updateEvent(selectedEvent.id, {
        name: editDraft.name,
        color: editDraft.color,
        description: editDraft.description,
      })
      await refreshEvents(true)
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setIsSaving(false)
    }
  }

  async function handleDeleteSelected() {
    if (!selectedEvent) return
    if (!window.confirm(`确定删除事件「${selectedEvent.name}」？（将同时删除其所有关联节点）`)) return
    setIsDeleting(true)
    setError('')
    try {
      await api.deleteEvent(selectedEvent.id)
      await refreshEvents(false)
      setSelectedEventIds([])
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '删除失败')
    } finally {
      setIsDeleting(false)
    }
  }

  async function handleInlineSave() {
    if (inlineEditEventId == null) return
    setInlineSaving(true)
    setError('')
    try {
      await api.updateEvent(inlineEditEventId, {
        name: inlineEditDraft.name,
        color: inlineEditDraft.color,
        description: inlineEditDraft.description,
      })
      // 本地即时更新，减少一次全量刷新带来的闪烁
      setEvents((prev) =>
        prev.map((e) =>
          e.id === inlineEditEventId
            ? { ...e, name: inlineEditDraft.name, color: inlineEditDraft.color, description: inlineEditDraft.description }
            : e,
        ),
      )
      setInlineEditEventId(null)
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setInlineSaving(false)
    }
  }

  function zoomIn() {
    setScale((v) => Math.min(v + 0.1, 2))
  }
  function zoomOut() {
    setScale((v) => Math.max(v - 0.1, 0.5))
  }
  function resetZoom() {
    setScale(1)
  }

  function startDrag(e: React.MouseEvent) {
    const grid = gridContainerRef.current
    if (!grid) return
    dragging.current = true
    dragStart.current = {
      x: e.pageX - grid.offsetLeft,
      y: e.pageY - grid.offsetTop,
      sl: grid.scrollLeft,
      st: grid.scrollTop,
    }
  }
  function onDrag(e: React.MouseEvent) {
    const grid = gridContainerRef.current
    if (!grid || !dragging.current) return
    e.preventDefault()
    const x = e.pageX - grid.offsetLeft
    const y = e.pageY - grid.offsetTop
    const walkX = (x - dragStart.current.x) * 1.5
    const walkY = (y - dragStart.current.y) * 1.5
    grid.scrollLeft = dragStart.current.sl - walkX
    grid.scrollTop = dragStart.current.st - walkY
  }
  function stopDrag() {
    dragging.current = false
  }

  function rebuildLines() {
    const content = contentContainerRef.current
    if (!content) return
    const containerRect = content.getBoundingClientRect()

    const activeEvents = (events || []).filter((e) => selectedEventIds.includes(e.id))
    const paths: LinePath[] = []

    activeEvents.forEach((evt, evtIndex) => {
      const relatedSceneIds = (evt.nodes || [])
        .filter((n) => n.target_type === 'scene')
        .map((n) => Number(n.target_id))

      const points: { x: number; y: number }[] = []
      for (const sceneId of relatedSceneIds) {
        const el = sceneRefs.current.get(sceneId)
        if (!el) continue
        const r = el.getBoundingClientRect()
        const cx = (r.left - containerRect.left + r.width / 2) / scale
        const cy = (r.top - containerRect.top + r.height / 2) / scale
        points.push({ x: cx, y: cy })
      }

      // 让线条略微错开避免完全重叠
      const offsetStep = 4
      const offset = (evtIndex - (activeEvents.length - 1) / 2) * offsetStep
      const sorted = points.slice().sort((a, b) => (a.y === b.y ? a.x - b.x : a.y - b.y))
      if (sorted.length < 2) return

      let d = `M ${sorted[0].x} ${sorted[0].y + offset}`
      for (let i = 1; i < sorted.length; i++) {
        const p = sorted[i]
        d += ` L ${p.x} ${p.y + offset}`
      }
      paths.push({ id: `evt-${evt.id}`, d, color: evt.color })
    })

    setLinePaths(paths)
  }

  // rebuild when selection/scale/data changes or on resize/scroll
  useEffect(() => {
    rebuildLines()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [events, selectedEventIds, episodes, scale])

  useEffect(() => {
    const grid = gridContainerRef.current
    if (!grid) return
    const onScroll = () => rebuildLines()
    grid.addEventListener('scroll', onScroll, { passive: true })
    const onResize = () => rebuildLines()
    window.addEventListener('resize', onResize)
    return () => {
      grid.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onResize)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scale, events, selectedEventIds])

  return (
    <div style={styles.page}>
      {/* 顶部控制区 */}
      <div style={styles.topbar}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <div style={styles.badge}>事件纵览 (Overview)</div>
          <select value={projectId ?? ''} onChange={(e) => setProjectId(e.target.value || null)} style={styles.select}>
            <option value="" disabled>
              选择项目…
            </option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <input
            value={eventQuery}
            onChange={(e) => setEventQuery(e.target.value)}
            style={{ ...styles.input, width: 240 }}
            placeholder="搜索事件（名称/描述）"
          />
          <select value={eventSort} onChange={(e) => setEventSort(e.target.value as any)} style={styles.select}>
            <option value="name_asc">名称 A→Z</option>
            <option value="name_desc">名称 Z→A</option>
            <option value="linked_desc">关联场 多→少</option>
            <option value="linked_asc">关联场 少→多</option>
          </select>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            {filteredSortedEvents.map((evt) => (
              <label
                key={evt.id}
                style={{
                  ...styles.eventTag,
                  borderColor: isEventSelected(evt.id) ? evt.color : 'rgba(229,231,235,0.25)',
                  background: isEventSelected(evt.id) ? 'rgba(255,255,255,0.04)' : 'rgba(255,255,255,0.02)',
                }}
              >
                <input
                  type="checkbox"
                  checked={isEventSelected(evt.id)}
                  onChange={(e) => {
                    const checked = e.target.checked
                    setSelectedEventIds((prev) => (checked ? Array.from(new Set([...prev, evt.id])) : prev.filter((x) => x !== evt.id)))
                  }}
                />
                <span style={{ width: 8, height: 8, borderRadius: 99, background: evt.color, display: 'inline-block' }} />
                <span style={{ opacity: 0.95 }}>{evt.name}</span>
                <button
                  type="button"
                  title="编辑描述"
                  style={styles.tagEditBtn}
                  onClick={(e) => {
                    e.preventDefault()
                    e.stopPropagation()
                    setInlineEditEventId(evt.id)
                  }}
                >
                  ✎
                </button>
              </label>
            ))}
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <button onClick={() => setShowCreate(true)} style={styles.btnPrimary}>
            + 新建事件
          </button>
          <button onClick={selectAll} style={styles.btn}>
            全选
          </button>
          <button onClick={clearSelection} style={styles.btn}>
            清空
          </button>
          <div style={{ width: 1, height: 16, background: 'rgba(255,255,255,0.12)' }} />
          {singleEventSelected ? (
            <Link to={editLink} style={{ ...styles.btnPrimary, textDecoration: 'none', display: 'inline-flex', gap: 6 }}>
              <span>Edit Detail</span>
              <span>→</span>
            </Link>
          ) : (
            <button style={{ ...styles.btnDisabled, cursor: 'not-allowed' }} title="请选择且仅选择一个事件">
              Edit Detail →
            </button>
          )}
        </div>
      </div>

      {error ? <div style={styles.error}>{error}</div> : null}

      {/* 单选编辑区：名称/颜色/描述/删除 */}
      {selectedEvent ? (
        <div style={styles.editor}>
          <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
            <div style={{ fontSize: 12, fontWeight: 900, opacity: 0.75 }}>当前事件</div>
            <input
              value={editDraft.name}
              onChange={(e) => setEditDraft((p) => ({ ...p, name: e.target.value }))}
              style={{ ...styles.input, width: 260, fontWeight: 800 }}
              placeholder="事件名称"
            />
            <input
              type="color"
              value={editDraft.color}
              onChange={(e) => setEditDraft((p) => ({ ...p, color: e.target.value }))}
              style={styles.color}
              title="事件颜色"
            />
            <button onClick={() => handleSaveSelected().catch(() => {})} style={styles.btn} disabled={isSaving}>
              {isSaving ? '保存中…' : '保存'}
            </button>
            <button onClick={() => handleDeleteSelected().catch(() => {})} style={styles.btnDanger} disabled={isDeleting}>
              {isDeleting ? '删除中…' : '删除事件'}
            </button>
          </div>
          <div style={{ marginTop: 10 }}>
            <div style={{ fontSize: 12, fontWeight: 900, opacity: 0.75, marginBottom: 6 }}>描述</div>
            <textarea
              value={editDraft.description}
              onChange={(e) => setEditDraft((p) => ({ ...p, description: e.target.value }))}
              style={styles.textarea}
              placeholder="事件描述（会保存到后端 Event.description）"
            />
          </div>
        </div>
      ) : null}

      {/* 新建事件弹窗 */}
      {showCreate ? (
        <div style={styles.modalMask} onClick={() => setShowCreate(false)}>
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 18, fontWeight: 900, marginBottom: 12 }}>新建事件</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={styles.smallLabel}>名称</div>
                <input
                  value={createForm.name}
                  onChange={(e) => setCreateForm((p) => ({ ...p, name: e.target.value }))}
                  style={styles.input}
                  placeholder="例如：张小凡黑化"
                />
              </div>
              <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
                <div style={styles.smallLabel}>颜色</div>
                <input
                  type="color"
                  value={createForm.color}
                  onChange={(e) => setCreateForm((p) => ({ ...p, color: e.target.value }))}
                  style={styles.color}
                />
              </div>
              <div>
                <div style={styles.smallLabel}>描述</div>
                <textarea
                  value={createForm.description}
                  onChange={(e) => setCreateForm((p) => ({ ...p, description: e.target.value }))}
                  style={{ ...styles.textarea, height: 120 }}
                  placeholder="可选：事件描述"
                />
              </div>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 14 }}>
              <button onClick={() => setShowCreate(false)} style={styles.btn}>
                取消
              </button>
              <button onClick={() => handleCreate().catch(() => {})} style={styles.btnPrimary} disabled={!createForm.name.trim()}>
                创建
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 事件列表：快速编辑描述（无需单选） */}
      {inlineEditEventId != null ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (inlineSaving) return
            setInlineEditEventId(null)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 18, fontWeight: 900, marginBottom: 12 }}>编辑事件</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={styles.smallLabel}>名称</div>
                <input
                  value={inlineEditDraft.name}
                  onChange={(e) => setInlineEditDraft((p) => ({ ...p, name: e.target.value }))}
                  style={styles.input}
                />
              </div>
              <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
                <div style={styles.smallLabel}>颜色</div>
                <input
                  type="color"
                  value={inlineEditDraft.color}
                  onChange={(e) => setInlineEditDraft((p) => ({ ...p, color: e.target.value }))}
                  style={styles.color}
                />
              </div>
              <div>
                <div style={styles.smallLabel}>描述</div>
                <textarea
                  value={inlineEditDraft.description}
                  onChange={(e) => setInlineEditDraft((p) => ({ ...p, description: e.target.value }))}
                  style={{ ...styles.textarea, height: 160 }}
                  placeholder="事件描述（会保存到后端 Event.description）"
                />
              </div>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 14 }}>
              <button
                onClick={() => {
                  if (inlineSaving) return
                  setInlineEditEventId(null)
                }}
                style={styles.btn}
              >
                取消
              </button>
              <button onClick={() => handleInlineSave().catch(() => {})} style={styles.btnPrimary} disabled={inlineSaving || !inlineEditDraft.name.trim()}>
                {inlineSaving ? '保存中…' : '保存'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 网格画布 */}
      <div ref={gridContainerRef} style={styles.grid} onMouseDown={startDrag} onMouseMove={onDrag} onMouseUp={stopDrag} onMouseLeave={stopDrag}>
        <div
          ref={contentContainerRef}
          style={{ display: 'inline-block', position: 'relative', minWidth: '100%', transform: `scale(${scale})`, transformOrigin: '0 0' }}
        >
          {/* 列标题 */}
          <div style={{ display: 'flex', marginBottom: 12, marginLeft: 60 }}>
            {Array.from({ length: maxSceneCount }, (_, i) => i + 1).map((i) => (
              <div key={i} style={{ width: 64, marginRight: 80, textAlign: 'center', fontSize: 12, opacity: 0.55 }}>
                scene {i}
              </div>
            ))}
          </div>

          {/* 行 */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 56 }}>
            {episodeRows.map((ep) => (
              <div key={ep.id} style={{ display: 'flex', alignItems: 'center' }}>
                <div style={{ width: 60, textAlign: 'right', paddingRight: 12, fontSize: 12, opacity: 0.8 }}>EP{ep.order}</div>
                <div style={{ display: 'flex', alignItems: 'center' }}>
                  {(ep.scenes || []).map((scene, idx) => (
                    <div
                      key={scene.id}
                      ref={(el) => setSceneRef(scene.id, el)}
                      style={{ width: 64, marginRight: 80, display: 'flex', justifyContent: 'center', position: 'relative' }}
                    >
                      <div
                        title={`${ep.title} - ${scene.title}\n${scene.action_text || ''}`}
                        style={{
                          width: 40,
                          height: 40,
                          borderRadius: 999,
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          fontSize: 11,
                          fontWeight: 800,
                          border: '1px solid rgba(255,255,255,0.10)',
                          ...(isSceneActive(scene.id)
                            ? { color: '#fff', boxShadow: '0 6px 18px rgba(0,0,0,0.25)', transform: 'scale(1.08)' }
                            : { background: 'rgba(255,255,255,0.04)', color: 'rgba(229,231,235,0.35)' }),
                          ...getSceneStyle(scene.id),
                          transition: 'transform 160ms ease',
                        }}
                      >
                        {!isSceneActive(scene.id) ? idx + 1 : ''}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {/* 连线层 */}
          <svg style={{ position: 'absolute', inset: 0, pointerEvents: 'none', overflow: 'visible', zIndex: 0 }}>
            {linePaths.map((line) => (
              <path key={line.id} d={line.d} stroke={line.color} strokeWidth={3} fill="none" strokeLinecap="round" strokeLinejoin="round" opacity={0.6} />
            ))}
          </svg>
        </div>
      </div>

      {/* 缩放控制 */}
      <div style={styles.zoom}>
        <button onClick={zoomIn} style={styles.zoomBtn}>
          +
        </button>
        <button onClick={zoomOut} style={styles.zoomBtn}>
          -
        </button>
        <button onClick={resetZoom} style={styles.zoomBtn}>
          1:1
        </button>
      </div>
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: { position: 'relative', height: 'calc(100vh - 32px)' },
  topbar: {
    background: 'rgba(255,255,255,0.04)',
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    padding: 12,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    flexWrap: 'wrap',
    marginBottom: 12,
  },
  badge: { fontSize: 12, fontWeight: 800, opacity: 0.75, textTransform: 'uppercase', letterSpacing: 0.5 },
  eventTag: { display: 'inline-flex', alignItems: 'center', gap: 6, padding: '6px 8px', borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', fontSize: 12, cursor: 'pointer', userSelect: 'none' },
  tagEditBtn: {
    marginLeft: 2,
    borderRadius: 8,
    border: '1px solid rgba(255,255,255,0.12)',
    background: 'rgba(0,0,0,0.20)',
    color: '#e5e7eb',
    padding: '2px 6px',
    cursor: 'pointer',
    fontSize: 12,
    lineHeight: '16px',
  },
  select: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '8px 10px', outline: 'none' },
  btn: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.14)', background: 'rgba(255,255,255,0.04)', color: '#e5e7eb', padding: '8px 10px', cursor: 'pointer' },
  btnPrimary: { borderRadius: 10, border: '1px solid rgba(99,102,241,0.6)', background: 'rgba(99,102,241,0.35)', color: '#fff', padding: '8px 10px', cursor: 'pointer' },
  btnDisabled: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(255,255,255,0.02)', color: 'rgba(229,231,235,0.45)', padding: '8px 10px' },
  btnDanger: {
    borderRadius: 10,
    border: '1px solid rgba(248,113,113,0.55)',
    background: 'rgba(248,113,113,0.20)',
    color: '#fff',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  grid: { position: 'relative', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 12, overflow: 'auto', height: 'calc(100vh - 170px)', padding: 24, cursor: 'grab' },
  zoom: { position: 'absolute', right: 12, bottom: 12, display: 'flex', flexDirection: 'column', gap: 6, padding: 6, borderRadius: 12, border: '1px solid rgba(255,255,255,0.08)', background: 'rgba(0,0,0,0.25)' },
  zoomBtn: { width: 36, height: 36, borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(255,255,255,0.04)', color: '#e5e7eb', cursor: 'pointer', fontWeight: 900 },
  editor: {
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    marginBottom: 12,
  },
  input: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '8px 10px',
    outline: 'none',
    boxSizing: 'border-box',
  },
  textarea: {
    width: '100%',
    height: 120,
    resize: 'vertical',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 10,
    boxSizing: 'border-box',
    outline: 'none',
  },
  color: { width: 34, height: 34, borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'transparent' },
  error: {
    marginTop: 10,
    marginBottom: 12,
    color: '#f87171',
    fontSize: 12,
  },
  modalMask: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0,0,0,0.55)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 50,
  },
  modal: {
    width: 460,
    borderRadius: 14,
    border: '1px solid rgba(255,255,255,0.10)',
    background: '#0b1220',
    padding: 16,
  },
  smallLabel: { fontSize: 12, fontWeight: 900, opacity: 0.75, marginBottom: 6 },
}


