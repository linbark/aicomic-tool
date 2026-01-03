import { useEffect, useMemo, useState } from 'react'
import api, { getApiBaseUrl } from '../api/client'
import type { EpisodeRead, SceneRead, ShotRead, SplitSceneItem, SplitShotItem } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

type SplitScenePreview = SplitSceneItem & { _key: string }
type SplitShotPreview = SplitShotItem & { _key: string }

type Selected =
  | { kind: 'episode'; episodeId: number }
  | { kind: 'scene'; episodeId: number; sceneId: number }
  | { kind: 'shot'; episodeId: number; sceneId: number; shotId: number }
  | { kind: 'none' }

export function ScriptPage() {
  const { projects, projectId, setProjectId, refreshProjects } = useProjectSelection()

  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [selected, setSelected] = useState<Selected>({ kind: 'none' })

  const [episodeDescription, setEpisodeDescription] = useState('')
  const [sceneDescription, setSceneDescription] = useState('')

  // 新建项目（避免使用 window.prompt，兼容 Tauri WebView）
  const [showCreateProject, setShowCreateProject] = useState(false)
  const [createProjectName, setCreateProjectName] = useState('')
  const [createProjectDesc, setCreateProjectDesc] = useState('')
  const [creatingProject, setCreatingProject] = useState(false)
  const [createProjectError, setCreateProjectError] = useState<string | null>(null)

  // 项目重命名/删除（兼容 Tauri WebView）
  const [showRenameProject, setShowRenameProject] = useState(false)
  const [renameProjectName, setRenameProjectName] = useState('')
  const [renameProjectDesc, setRenameProjectDesc] = useState('')
  const [renamingProject, setRenamingProject] = useState(false)
  const [renameProjectError, setRenameProjectError] = useState<string | null>(null)

  const [showDeleteProject, setShowDeleteProject] = useState(false)
  const [deletingProject, setDeletingProject] = useState(false)
  const [deleteProjectError, setDeleteProjectError] = useState<string | null>(null)

  // 新建集（避免使用 window.prompt，兼容 Tauri WebView）
  const [showCreateEpisode, setShowCreateEpisode] = useState(false)
  const [createEpisodeTitle, setCreateEpisodeTitle] = useState('')
  const [createEpisodeError, setCreateEpisodeError] = useState<string | null>(null)
  const [creatingEpisode, setCreatingEpisode] = useState(false)

  const [splitScenesPreview, setSplitScenesPreview] = useState<SplitScenePreview[]>([])
  const [isSplitting, setIsSplitting] = useState(false)
  const [splitError, setSplitError] = useState<string | null>(null)
  const [overwriteOnImport, setOverwriteOnImport] = useState(false)
  const [isImporting, setIsImporting] = useState(false)

  const [storyboardPreview, setStoryboardPreview] = useState<SplitShotPreview[]>([])
  const [isStoryboardSplitting, setIsStoryboardSplitting] = useState(false)
  const [storyboardError, setStoryboardError] = useState<string | null>(null)
  const [overwriteShotsOnImport, setOverwriteShotsOnImport] = useState(false)
  const [isStoryboardImporting, setIsStoryboardImporting] = useState(false)

  const [shotDraft, setShotDraft] = useState<Partial<ShotRead> | null>(null)

  // AI 写作：大纲优化 / 剧本生成（结果预览后再应用）
  const [aiWritingBusy, setAiWritingBusy] = useState<'outline' | 'script' | null>(null)
  const [aiWritingError, setAiWritingError] = useState<string | null>(null)
  const [aiResult, setAiResult] = useState<{ title: string; text: string } | null>(null)

  // 项目列表与 projectId 由 useProjectSelection 统一管理（含 localStorage 记忆）

  const selectedEpisode = useMemo(() => {
    if (selected.kind === 'episode') return episodes.find((e) => e.id === selected.episodeId) || null
    if (selected.kind === 'scene' || selected.kind === 'shot')
      return episodes.find((e) => e.id === selected.episodeId) || null
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

  async function refreshScript(nextProjectId = projectId) {
    if (!nextProjectId) return
    const res = await api.getScript(nextProjectId)
    setEpisodes(res.data || [])
    setSelected({ kind: 'none' })
    setEpisodeDescription('')
    setSceneDescription('')
    setSplitScenesPreview([])
    setStoryboardPreview([])
    setShotDraft(null)
  }

  const currentProject = useMemo(() => {
    if (!projectId) return null
    return projects.find((p) => p.id === projectId) || null
  }, [projects, projectId])

  useEffect(() => {
    refreshScript().catch(() => {
      // ignore
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId])

  // 同步选中对象的 description 到右侧编辑区
  useEffect(() => {
    if (selected.kind === 'episode') {
      const ep = episodes.find((e) => e.id === selected.episodeId)
      setEpisodeDescription(ep?.description || '')
      setSplitScenesPreview([])
      setSplitError(null)
      return
    }
    if (selected.kind === 'scene') {
      const ep = episodes.find((e) => e.id === selected.episodeId)
      const sc = ep?.scenes?.find((s) => s.id === selected.sceneId)
      setSceneDescription(sc?.description || '')
      setStoryboardPreview([])
      setStoryboardError(null)
      return
    }
    if (selected.kind === 'shot') {
      const sh = selectedShot
      if (sh) setShotDraft(JSON.parse(JSON.stringify(sh)) as ShotRead)
      return
    }
    setShotDraft(null)
  }, [episodes, selected, selectedShot])

  const nextEpisodeOrder = useMemo(() => {
    return episodes.length > 0 ? Math.max(...episodes.map((e) => e.order || 0)) + 1 : 1
  }, [episodes])

  async function createProject() {
    const name = (createProjectName || '').trim()
    if (!name) return
    setCreatingProject(true)
    setCreateProjectError(null)
    try {
      const res = await api.createProject({ name, description: (createProjectDesc || '').trim() || null })
      const newId = res.data?.id
      await refreshProjects()
      if (typeof newId === 'number') setProjectId(newId)
      setShowCreateProject(false)
      setCreateProjectName('')
      setCreateProjectDesc('')
    } catch (e: any) {
      setCreateProjectError(e?.response?.data?.detail || e?.message || '新建项目失败')
    } finally {
      setCreatingProject(false)
    }
  }

  async function updateProjectMeta() {
    if (!projectId) return
    const name = (renameProjectName || '').trim()
    if (!name) return
    setRenamingProject(true)
    setRenameProjectError(null)
    try {
      await api.updateProject(projectId, { name, description: (renameProjectDesc || '').trim() || null })
      await refreshProjects()
      setShowRenameProject(false)
    } catch (e: any) {
      setRenameProjectError(e?.response?.data?.detail || e?.message || '重命名失败')
    } finally {
      setRenamingProject(false)
    }
  }

  async function deleteCurrentProject() {
    if (!projectId) return
    setDeletingProject(true)
    setDeleteProjectError(null)
    try {
      await api.deleteProject(projectId)
      // 先刷新项目列表（会自动选择第一个并同步 localStorage）
      await refreshProjects()
      setShowDeleteProject(false)
      // 清一下本页数据（后续 useEffect 会根据新的 projectId 自动加载）
      setEpisodes([])
      setSelected({ kind: 'none' })
      setEpisodeDescription('')
      setSceneDescription('')
      setSplitScenesPreview([])
      setStoryboardPreview([])
      setShotDraft(null)
    } catch (e: any) {
      setDeleteProjectError(e?.response?.data?.detail || e?.message || '删除失败')
    } finally {
      setDeletingProject(false)
    }
  }

  async function createEpisode() {
    if (!projectId) return
    const title = (createEpisodeTitle || '').trim()
    if (!title) return
    setCreatingEpisode(true)
    setCreateEpisodeError(null)
    try {
      const res = await api.createEpisode(projectId, { title, order: nextEpisodeOrder })
      const newId = (res.data as any)?.id
      await refreshScript(projectId)
      if (typeof newId === 'number') setSelected({ kind: 'episode', episodeId: newId })
      setShowCreateEpisode(false)
      setCreateEpisodeTitle('')
    } catch (e: any) {
      setCreateEpisodeError(e?.response?.data?.detail || e?.message || '新建失败')
    } finally {
      setCreatingEpisode(false)
    }
  }

  async function addScene(episodeId: number) {
    const title = window.prompt('新建一场：标题', '新场景')
    if (!title) return
    const ep = episodes.find((e) => e.id === episodeId)
    const nextSeq =
      (ep?.scenes?.length || 0) > 0 ? Math.max(...(ep?.scenes || []).map((s) => s.sequence_number || 0)) + 1 : 1
    await api.createScene(episodeId, { title, sequence_number: nextSeq })
    await refreshScript(projectId)
  }

  async function addShot(sceneId: number) {
    const sc = selectedScene
    const nextSeq =
      (sc?.shots?.length || 0) > 0 ? Math.max(...(sc?.shots || []).map((s) => s.sequence_number || 0)) + 1 : 1
    await api.createShot(sceneId, { sequence_number: nextSeq, title: `镜头 ${nextSeq}`, action_text: '' })
    await refreshScript(projectId)
  }

  async function saveEpisodeScript() {
    if (selected.kind !== 'episode') return
    await api.updateEpisode(selected.episodeId, { description: episodeDescription })
    await refreshScript(projectId)
    setSelected({ kind: 'episode', episodeId: selected.episodeId })
  }

  async function saveSceneScript() {
    if (selected.kind !== 'scene') return
    await api.updateScene(selected.sceneId, { description: sceneDescription })
    await refreshScript(projectId)
    setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selected.sceneId })
  }

  async function saveShot() {
    if (selected.kind !== 'shot' || !shotDraft) return
    await api.updateShot(selected.shotId, {
      title: shotDraft.title,
      action_text: shotDraft.action_text,
      dialogue: shotDraft.dialogue,
      prompt: shotDraft.prompt,
      status: shotDraft.status,
    })
    await refreshScript(projectId)
    setSelected({ kind: 'shot', episodeId: selected.episodeId, sceneId: selected.sceneId, shotId: selected.shotId })
  }

  async function handleDeleteEpisode(episodeId: number) {
    if (!window.confirm('确定删除该集？')) return
    await api.deleteEpisode(episodeId)
    await refreshScript(projectId)
  }

  async function handleDeleteScene(sceneId: number) {
    if (!window.confirm('确定删除该场？（会同时清理场下镜头与文件夹）')) return
    await api.deleteScene(sceneId)
    await refreshScript(projectId)
  }

  async function handleDeleteShot(shotId: number) {
    if (!window.confirm('确定删除该镜头？（会同时删除素材文件）')) return
    await api.deleteShot(shotId)
    await refreshScript(projectId)
  }

  async function handleAutoSplit() {
    if (selected.kind !== 'episode') return
    const text = (episodeDescription || '').trim()
    if (!text) return
    setIsSplitting(true)
    setSplitError(null)
    try {
      const res = await api.aiSplitScenes({ text })
      const list = res.data || []
      setSplitScenesPreview(
        list.map((it, idx) => ({
          _key: `${Date.now()}_${idx}`,
          title: it.title,
          description: it.description,
        })),
      )
    } catch (e: any) {
      setSplitError(e?.response?.data?.detail || e?.message || '自动分场失败')
    } finally {
      setIsSplitting(false)
    }
  }

  async function handleOutlineOptimize() {
    if (selected.kind !== 'episode') return
    const text = (episodeDescription || '').trim()
    if (!text) return
    setAiWritingBusy('outline')
    setAiWritingError(null)
    try {
      const res = await api.aiOutlineOptimize({ text })
      setAiResult({ title: '大纲优化结果', text: res.data?.text || '' })
    } catch (e: any) {
      setAiWritingError(e?.response?.data?.detail || e?.message || '大纲优化失败')
    } finally {
      setAiWritingBusy(null)
    }
  }

  async function handleGenerateScript() {
    if (selected.kind !== 'episode') return
    const text = (episodeDescription || '').trim()
    if (!text) return
    setAiWritingBusy('script')
    setAiWritingError(null)
    try {
      const res = await api.aiGenerateScript({ text })
      setAiResult({ title: '剧本生成结果', text: res.data?.text || '' })
    } catch (e: any) {
      setAiWritingError(e?.response?.data?.detail || e?.message || '剧本生成失败')
    } finally {
      setAiWritingBusy(null)
    }
  }

  async function handleImportScenes() {
    if (selected.kind !== 'episode') return
    const ep = selectedEpisode
    if (!ep) return
    if (splitScenesPreview.length === 0) return
    setIsImporting(true)
    try {
      if (overwriteOnImport && ep.scenes?.length) {
        for (const sc of ep.scenes) {
          await api.deleteScene(sc.id)
        }
      }
      // 逐个创建场（create_scene 当前仅写 title/sequence_number，所以再 patch 写 description）
      for (let i = 0; i < splitScenesPreview.length; i++) {
        const sc = splitScenesPreview[i]
        const createRes = await api.createScene(ep.id, { title: sc.title, sequence_number: i + 1 })
        const created = createRes.data as SceneRead
        await api.updateScene(created.id, { description: sc.description })
      }
      await refreshScript(projectId)
      setSelected({ kind: 'episode', episodeId: ep.id })
    } finally {
      setIsImporting(false)
    }
  }

  async function handleAutoStoryboard() {
    if (selected.kind !== 'scene') return
    const text = (sceneDescription || '').trim()
    if (!text) return
    setIsStoryboardSplitting(true)
    setStoryboardError(null)
    try {
      const res = await api.aiSplitShots({ text })
      const list = res.data || []
      setStoryboardPreview(
        list.map((it, idx) => ({
          _key: `${Date.now()}_${idx}`,
          title: it.title ?? null,
          action_text: it.action_text,
        })),
      )
    } catch (e: any) {
      setStoryboardError(e?.response?.data?.detail || e?.message || '自动分镜失败')
    } finally {
      setIsStoryboardSplitting(false)
    }
  }

  async function handleImportShots() {
    if (selected.kind !== 'scene') return
    const sc = selectedScene
    if (!sc) return
    if (storyboardPreview.length === 0) return
    setIsStoryboardImporting(true)
    try {
      if (overwriteShotsOnImport && sc.shots?.length) {
        for (const sh of sc.shots) {
          await api.deleteShot(sh.id)
        }
      }
      for (let i = 0; i < storyboardPreview.length; i++) {
        const sh = storyboardPreview[i]
        await api.createShot(sc.id, {
          sequence_number: i + 1,
          title: sh.title || `镜头 ${i + 1}`,
          action_text: sh.action_text,
          status: 'draft',
        })
      }
      await refreshScript(projectId)
      setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: sc.id })
    } finally {
      setIsStoryboardImporting(false)
    }
  }

  return (
    <div style={styles.page}>
      <div style={styles.topbar}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{ fontWeight: 700 }}>剧本</div>
          <span style={{ opacity: 0.7, fontSize: 12 }}>API：{getApiBaseUrl()}</span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <select
            value={projectId ?? ''}
            onChange={(e) => setProjectId(e.target.value ? Number(e.target.value) : null)}
            style={styles.select}
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
          <button
            onClick={() => {
              setCreateProjectError(null)
              setCreateProjectName('')
              setCreateProjectDesc('')
              setShowCreateProject(true)
            }}
            style={styles.btn}
          >
            + 新建项目
          </button>
          <button
            onClick={() => {
              if (!projectId) return
              setRenameProjectError(null)
              setRenameProjectName(currentProject?.name || '')
              setRenameProjectDesc(currentProject?.description || '')
              setShowRenameProject(true)
            }}
            style={styles.btn}
            disabled={!projectId}
          >
            重命名
          </button>
          <button
            onClick={() => {
              if (!projectId) return
              setDeleteProjectError(null)
              setShowDeleteProject(true)
            }}
            style={styles.btnDanger}
            disabled={!projectId}
            title="删除项目（含项目文件夹与关联数据）"
          >
            删除项目
          </button>
          <button onClick={() => refreshScript(projectId).catch(() => {})} style={styles.btn}>
            刷新
          </button>
          <button
            onClick={() => {
              if (!projectId) return
              setCreateEpisodeError(null)
              setCreateEpisodeTitle(`第${nextEpisodeOrder}集`)
              setShowCreateEpisode(true)
            }}
            style={styles.btnPrimary}
            disabled={!projectId}
          >
            + 新建集
          </button>
        </div>
      </div>

      <div style={styles.body}>
        {/* 左：Episode 列表 */}
        <div style={styles.colLeft}>
          <div style={styles.colHeader}>
            <div style={styles.colTitle}>Episodes</div>
          </div>
          <div style={styles.colScroll}>
            {episodes.map((ep) => (
              <div
                key={ep.id}
                style={{
                  ...styles.card,
                  ...(selected.kind !== 'none' && selectedEpisode?.id === ep.id ? styles.cardActive : null),
                }}
                onClick={() => setSelected({ kind: 'episode', episodeId: ep.id })}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
                  <div style={{ minWidth: 0 }}>
                    <div style={styles.cardTitle}>
                      <span style={styles.mono}>EP{ep.order}</span> {ep.title}
                    </div>
                    {ep.description ? <div style={styles.cardSub}>{ep.description}</div> : null}
                  </div>
                  <button
                    style={styles.iconBtn}
                    onClick={(e) => {
                      e.stopPropagation()
                      handleDeleteEpisode(ep.id).catch(() => {})
                    }}
                    title="删除本集"
                  >
                    🗑️
                  </button>
                </div>
                <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                  <button
                    style={styles.smallBtn}
                    onClick={(e) => {
                      e.stopPropagation()
                      addScene(ep.id).catch(() => {})
                    }}
                  >
                    + 加场
                  </button>
                </div>
              </div>
            ))}
            {episodes.length === 0 ? <div style={styles.empty}>暂无集，点击右上角“+ 新建集”</div> : null}
          </div>
        </div>

        {/* 中：Scene 或 Shot 列表 */}
        <div style={styles.colMid}>
          <div style={styles.colHeader}>
            <div style={styles.colTitle}>
              {selectedScene ? 'Shots' : selectedEpisode ? 'Scenes' : '请选择左侧一集'}
            </div>
          </div>
          <div style={styles.colScroll}>
            {/* Episode 选中且未选 Scene：显示 Scenes */}
            {selected.kind === 'episode' && selectedEpisode ? (
              <>
                {(selectedEpisode.scenes || []).map((sc) => (
                  <div
                    key={sc.id}
                    style={{
                      ...styles.card,
                    }}
                    onClick={() => setSelected({ kind: 'scene', episodeId: selectedEpisode.id, sceneId: sc.id })}
                  >
                    <div style={styles.cardTitle}>
                      <span style={styles.mono}>SC{sc.sequence_number}</span> {sc.title}
                    </div>
                    {sc.description ? <div style={styles.cardSub}>{sc.description}</div> : null}
                  </div>
                ))}
                {(selectedEpisode.scenes || []).length === 0 ? (
                  <div style={styles.empty}>暂无场，点击左侧“+ 加场”</div>
                ) : null}
              </>
            ) : null}

            {/* Scene 选中：显示 Shots */}
            {selected.kind === 'scene' && selectedScene ? (
              <>
                <div style={{ display: 'flex', gap: 8, marginBottom: 10 }}>
                  <button style={styles.btnPrimary} onClick={() => addShot(selectedScene.id).catch(() => {})}>
                    + 加镜
                  </button>
                  <button style={styles.btnDanger} onClick={() => handleDeleteScene(selectedScene.id).catch(() => {})}>
                    删除本场
                  </button>
                </div>
                {(selectedScene.shots || []).map((sh) => (
                  <div
                    key={sh.id}
                    style={{
                      ...styles.card,
                    }}
                    onClick={() =>
                      setSelected({
                        kind: 'shot',
                        episodeId: selected.episodeId,
                        sceneId: selectedScene.id,
                        shotId: sh.id,
                      })
                    }
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10 }}>
                      <div style={{ minWidth: 0 }}>
                        <div style={styles.cardTitle}>
                          <span style={styles.mono}>#{sh.sequence_number}</span> {sh.title || '（无标题）'}
                        </div>
                        {sh.action_text ? <div style={styles.cardSub}>{sh.action_text}</div> : null}
                      </div>
                      <button
                        style={styles.iconBtn}
                        title="删除镜头"
                        onClick={(e) => {
                          e.stopPropagation()
                          handleDeleteShot(sh.id).catch(() => {})
                        }}
                      >
                        ×
                      </button>
                    </div>
                  </div>
                ))}
                {(selectedScene.shots || []).length === 0 ? <div style={styles.empty}>暂无镜头，点击“+ 加镜”</div> : null}
              </>
            ) : null}

            {/* Shot 选中：左侧仍然展示当前 scene 的 shots，避免迷路 */}
            {selected.kind === 'shot' && selectedScene ? (
              <>
                <div style={{ display: 'flex', gap: 8, marginBottom: 10 }}>
                  <button
                    style={styles.btn}
                    onClick={() => setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selectedScene.id })}
                  >
                    ← 返回本场
                  </button>
                </div>
                {(selectedScene.shots || []).map((sh) => (
                  <div
                    key={sh.id}
                    style={{
                      ...styles.card,
                      ...(selected.kind === 'shot' && selectedShot?.id === sh.id ? styles.cardActive : null),
                    }}
                    onClick={() =>
                      setSelected({
                        kind: 'shot',
                        episodeId: selected.episodeId,
                        sceneId: selectedScene.id,
                        shotId: sh.id,
                      })
                    }
                  >
                    <div style={styles.cardTitle}>
                      <span style={styles.mono}>#{sh.sequence_number}</span> {sh.title || '（无标题）'}
                    </div>
                    {sh.action_text ? <div style={styles.cardSub}>{sh.action_text}</div> : null}
                  </div>
                ))}
              </>
            ) : null}
          </div>
        </div>

        {/* 右：详情编辑 */}
        <div style={styles.colRight}>
          <div style={styles.colHeader}>
            <div style={styles.colTitle}>详情</div>
          </div>
          <div style={styles.colScroll}>
            {/* Episode 编辑 */}
            {selected.kind === 'episode' && selectedEpisode ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    <span style={styles.mono}>EP{selectedEpisode.order}</span> {selectedEpisode.title}
                  </div>
                  <button style={styles.btnPrimary} onClick={() => saveEpisodeScript().catch(() => {})}>
                    保存修改
                  </button>
                </div>

                <div style={styles.labelRow}>
                  <div style={styles.label}>剧本内容</div>
                  <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                    <button
                      style={styles.btn}
                      onClick={() => handleOutlineOptimize().catch(() => {})}
                      disabled={aiWritingBusy !== null || !episodeDescription.trim()}
                      title="对当前文本进行大纲优化（可在 Prompt 模板中配置）"
                    >
                      {aiWritingBusy === 'outline' ? '优化中…' : '优化大纲'}
                    </button>
                    <button
                      style={styles.btn}
                      onClick={() => handleGenerateScript().catch(() => {})}
                      disabled={aiWritingBusy !== null || !episodeDescription.trim()}
                      title="根据当前文本生成剧本（可在 Prompt 模板中配置）"
                    >
                      {aiWritingBusy === 'script' ? '生成中…' : '生成剧本'}
                    </button>
                    <button
                      style={styles.btn}
                      onClick={() => handleAutoSplit().catch(() => {})}
                      disabled={isSplitting || !episodeDescription.trim()}
                      title="自动分场"
                    >
                      {isSplitting ? '分场中…' : '自动分场'}
                    </button>
                  </div>
                </div>

                <textarea
                  value={episodeDescription}
                  onChange={(e) => setEpisodeDescription(e.target.value)}
                  style={styles.textarea}
                  placeholder="编写本集的剧本内容…"
                />

                <div style={{ marginTop: 12 }}>
                  {aiWritingError ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }}>{aiWritingError}</div> : null}
                  <div style={styles.labelRow}>
                    <div style={styles.label}>分场预览 {splitScenesPreview.length ? `(${splitScenesPreview.length} 场)` : ''}</div>
                    <label style={{ fontSize: 12, opacity: 0.8, display: 'flex', alignItems: 'center', gap: 6 }}>
                      <input type="checkbox" checked={overwriteOnImport} onChange={(e) => setOverwriteOnImport(e.target.checked)} />
                      覆盖导入（先清空本集已有场）
                    </label>
                    <button
                      style={styles.btnPrimary}
                      onClick={() => handleImportScenes().catch(() => {})}
                      disabled={isImporting || splitScenesPreview.length === 0}
                      title="一键导入为场"
                    >
                      {isImporting ? '导入中…' : '一键导入'}
                    </button>
                  </div>
                  {splitError ? <div style={{ color: '#f87171', fontSize: 12 }}>{splitError}</div> : null}
                  {splitScenesPreview.length === 0 ? (
                    <div style={styles.emptyBox}>点击“自动分场”生成预览，然后点击“一键导入”创建场景。</div>
                  ) : (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                      {splitScenesPreview.map((sc, idx) => (
                        <div key={sc._key} style={styles.previewCard}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                            <span style={styles.miniMono}>SC{idx + 1}</span>
                            <input
                              value={sc.title}
                              onChange={(e) => {
                                const v = e.target.value
                                setSplitScenesPreview((prev) => prev.map((x) => (x._key === sc._key ? { ...x, title: v } : x)))
                              }}
                              style={styles.input}
                              placeholder="场标题"
                            />
                          </div>
                          <textarea
                            value={sc.description}
                            onChange={(e) => {
                              const v = e.target.value
                              setSplitScenesPreview((prev) =>
                                prev.map((x) => (x._key === sc._key ? { ...x, description: v } : x)),
                              )
                            }}
                            style={{ ...styles.textarea, height: 120 }}
                            placeholder="该场的文本内容…"
                          />
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </section>
            ) : null}

            {/* Scene 编辑 */}
            {selected.kind === 'scene' && selectedScene ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    <span style={styles.mono}>SC{selectedScene.sequence_number}</span> {selectedScene.title}
                  </div>
                  <button style={styles.btnPrimary} onClick={() => saveSceneScript().catch(() => {})}>
                    保存修改
                  </button>
                </div>

                <div style={styles.labelRow}>
                  <div style={styles.label}>剧本内容</div>
                  <button
                    style={styles.btn}
                    onClick={() => handleAutoStoryboard().catch(() => {})}
                    disabled={isStoryboardSplitting || !sceneDescription.trim()}
                    title="自动分镜"
                  >
                    {isStoryboardSplitting ? '分镜中…' : '自动分镜'}
                  </button>
                </div>

                <textarea
                  value={sceneDescription}
                  onChange={(e) => setSceneDescription(e.target.value)}
                  style={styles.textarea}
                  placeholder="编写本场的剧本内容…"
                />

                <div style={{ marginTop: 12 }}>
                  <div style={styles.labelRow}>
                    <div style={styles.label}>分镜预览 {storyboardPreview.length ? `(${storyboardPreview.length} 镜)` : ''}</div>
                    <label style={{ fontSize: 12, opacity: 0.8, display: 'flex', alignItems: 'center', gap: 6 }}>
                      <input
                        type="checkbox"
                        checked={overwriteShotsOnImport}
                        onChange={(e) => setOverwriteShotsOnImport(e.target.checked)}
                      />
                      覆盖导入（先清空本场已有镜头）
                    </label>
                    <button
                      style={styles.btnPrimary}
                      onClick={() => handleImportShots().catch(() => {})}
                      disabled={isStoryboardImporting || storyboardPreview.length === 0}
                      title="一键导入为镜头"
                    >
                      {isStoryboardImporting ? '导入中…' : '一键导入'}
                    </button>
                  </div>
                  {storyboardError ? <div style={{ color: '#f87171', fontSize: 12 }}>{storyboardError}</div> : null}
                  {storyboardPreview.length === 0 ? (
                    <div style={styles.emptyBox}>点击“自动分镜”生成预览，然后点击“一键导入”创建镜头。</div>
                  ) : (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                      {storyboardPreview.map((sh, idx) => (
                        <div key={sh._key} style={styles.previewCard}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                            <span style={styles.miniMono}>SHOT{idx + 1}</span>
                            <input
                              value={sh.title || ''}
                              onChange={(e) => {
                                const v = e.target.value
                                setStoryboardPreview((prev) => prev.map((x) => (x._key === sh._key ? { ...x, title: v } : x)))
                              }}
                              style={styles.input}
                              placeholder="镜头标题（可选）"
                            />
                          </div>
                          <textarea
                            value={sh.action_text}
                            onChange={(e) => {
                              const v = e.target.value
                              setStoryboardPreview((prev) =>
                                prev.map((x) => (x._key === sh._key ? { ...x, action_text: v } : x)),
                              )
                            }}
                            style={{ ...styles.textarea, height: 120 }}
                            placeholder="镜头描述…"
                          />
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </section>
            ) : null}

            {/* Shot 编辑 */}
            {selected.kind === 'shot' && shotDraft ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700 }}>
                    <span style={styles.mono}>SHOT</span> #{shotDraft.sequence_number}
                  </div>
                  <button style={styles.btnPrimary} onClick={() => saveShot().catch(() => {})}>
                    保存镜头
                  </button>
                </div>

                <div style={{ display: 'grid', gridTemplateColumns: '1fr 160px', gap: 10 }}>
                  <div>
                    <div style={styles.label}>标题</div>
                    <input
                      value={shotDraft.title || ''}
                      onChange={(e) => setShotDraft((p) => ({ ...(p || {}), title: e.target.value }))}
                      style={styles.input}
                      placeholder="镜头标题"
                    />
                  </div>
                  <div>
                    <div style={styles.label}>状态</div>
                    <select
                      value={shotDraft.status || 'draft'}
                      onChange={(e) => setShotDraft((p) => ({ ...(p || {}), status: e.target.value }))}
                      style={styles.select}
                    >
                      <option value="draft">draft</option>
                      <option value="todo">todo</option>
                      <option value="doing">doing</option>
                      <option value="done">done</option>
                    </select>
                  </div>
                </div>

                <div style={{ marginTop: 10 }}>
                  <div style={styles.label}>动作文本</div>
                  <textarea
                    value={shotDraft.action_text || ''}
                    onChange={(e) => setShotDraft((p) => ({ ...(p || {}), action_text: e.target.value }))}
                    style={styles.textarea}
                    placeholder="镜头动作/画面描述…"
                  />
                </div>
                <div style={{ marginTop: 10 }}>
                  <div style={styles.label}>对白</div>
                  <textarea
                    value={shotDraft.dialogue || ''}
                    onChange={(e) => setShotDraft((p) => ({ ...(p || {}), dialogue: e.target.value }))}
                    style={{ ...styles.textarea, height: 120 }}
                    placeholder="对白…"
                  />
                </div>
                <div style={{ marginTop: 10 }}>
                  <div style={styles.label}>Prompt</div>
                  <textarea
                    value={shotDraft.prompt || ''}
                    onChange={(e) => setShotDraft((p) => ({ ...(p || {}), prompt: e.target.value }))}
                    style={{ ...styles.textarea, height: 120 }}
                    placeholder="生成提示词…"
                  />
                </div>
              </section>
            ) : null}

            {selected.kind === 'none' ? <div style={styles.empty}>从左侧选择一集开始。</div> : null}
          </div>
        </div>
      </div>

      {/* AI 结果预览弹窗（应用到文本前先预览） */}
      {aiResult ? (
        <div style={styles.modalMask} onClick={() => setAiResult(null)}>
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>{aiResult.title}</div>
              <button style={styles.btn} onClick={() => setAiResult(null)}>
                关闭
              </button>
            </div>
            <textarea value={aiResult.text} readOnly style={{ ...styles.textarea, height: 420 }} />
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 10 }}>
              <button
                style={styles.btn}
                onClick={() => {
                  navigator.clipboard?.writeText(aiResult.text || '').catch(() => {})
                }}
              >
                复制
              </button>
              <button
                style={styles.btnPrimary}
                onClick={() => {
                  setEpisodeDescription(aiResult.text || '')
                  setAiResult(null)
                }}
              >
                应用到当前文本
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 新建集弹窗 */}
      {showCreateEpisode ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (creatingEpisode) return
            setShowCreateEpisode(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>新建一集</div>
              <button style={styles.btn} onClick={() => setShowCreateEpisode(false)} disabled={creatingEpisode}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div style={{ fontSize: 12, opacity: 0.75 }}>将创建：EP{nextEpisodeOrder}</div>
              <input
                value={createEpisodeTitle}
                onChange={(e) => setCreateEpisodeTitle(e.target.value)}
                style={styles.input}
                placeholder="请输入标题"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter') createEpisode().catch(() => {})
                }}
              />
              {createEpisodeError ? <div style={{ color: '#f87171', fontSize: 12 }}>{createEpisodeError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowCreateEpisode(false)} disabled={creatingEpisode}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => createEpisode().catch(() => {})} disabled={creatingEpisode || !createEpisodeTitle.trim()}>
                {creatingEpisode ? '创建中…' : '创建'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 新建项目弹窗 */}
      {showCreateProject ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (creatingProject) return
            setShowCreateProject(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>新建项目</div>
              <button style={styles.btn} onClick={() => setShowCreateProject(false)} disabled={creatingProject}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>项目名称</div>
                <input
                  value={createProjectName}
                  onChange={(e) => setCreateProjectName(e.target.value)}
                  style={styles.input}
                  placeholder="例如：第一季"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') createProject().catch(() => {})
                  }}
                />
              </div>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>描述（可选）</div>
                <textarea
                  value={createProjectDesc}
                  onChange={(e) => setCreateProjectDesc(e.target.value)}
                  style={{ ...styles.textarea, height: 140 }}
                  placeholder="项目简介…"
                />
              </div>
              {createProjectError ? <div style={{ color: '#f87171', fontSize: 12 }}>{createProjectError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowCreateProject(false)} disabled={creatingProject}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => createProject().catch(() => {})} disabled={creatingProject || !createProjectName.trim()}>
                {creatingProject ? '创建中…' : '创建'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 重命名项目弹窗 */}
      {showRenameProject ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (renamingProject) return
            setShowRenameProject(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>重命名项目</div>
              <button style={styles.btn} onClick={() => setShowRenameProject(false)} disabled={renamingProject}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>项目名称</div>
                <input
                  value={renameProjectName}
                  onChange={(e) => setRenameProjectName(e.target.value)}
                  style={styles.input}
                  placeholder="项目名称"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') updateProjectMeta().catch(() => {})
                  }}
                />
              </div>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>描述（可选）</div>
                <textarea
                  value={renameProjectDesc}
                  onChange={(e) => setRenameProjectDesc(e.target.value)}
                  style={{ ...styles.textarea, height: 140 }}
                  placeholder="项目简介…"
                />
              </div>
              {renameProjectError ? <div style={{ color: '#f87171', fontSize: 12 }}>{renameProjectError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowRenameProject(false)} disabled={renamingProject}>
                取消
              </button>
              <button
                style={styles.btnPrimary}
                onClick={() => updateProjectMeta().catch(() => {})}
                disabled={renamingProject || !renameProjectName.trim()}
              >
                {renamingProject ? '保存中…' : '保存'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 删除项目确认弹窗 */}
      {showDeleteProject ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (deletingProject) return
            setShowDeleteProject(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontWeight: 900, marginBottom: 10 }}>删除项目</div>
            <div style={{ fontSize: 12, opacity: 0.85, lineHeight: 1.6 }}>
              将删除项目「{currentProject?.name || ''}」及其所有关联数据与本地文件夹。此操作不可撤销。
            </div>
            {deleteProjectError ? <div style={{ color: '#f87171', fontSize: 12, marginTop: 10 }}>{deleteProjectError}</div> : null}
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 14 }}>
              <button style={styles.btn} onClick={() => setShowDeleteProject(false)} disabled={deletingProject}>
                取消
              </button>
              <button style={styles.btnDanger} onClick={() => deleteCurrentProject().catch(() => {})} disabled={deletingProject}>
                {deletingProject ? '删除中…' : '确认删除'}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  topbar: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.04)',
  },
  body: {
    display: 'grid',
    gridTemplateColumns: '320px 320px 1fr',
    gap: 12,
    minHeight: 'calc(100vh - 140px)',
  },
  colLeft: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    overflow: 'hidden',
    background: 'rgba(255,255,255,0.03)',
  },
  colMid: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    overflow: 'hidden',
    background: 'rgba(255,255,255,0.03)',
  },
  colRight: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    overflow: 'hidden',
    background: 'rgba(255,255,255,0.03)',
  },
  colHeader: {
    padding: '10px 12px',
    borderBottom: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(0,0,0,0.12)',
  },
  colTitle: {
    fontWeight: 700,
    fontSize: 13,
    opacity: 0.9,
  },
  colScroll: {
    padding: 12,
    overflow: 'auto',
    maxHeight: 'calc(100vh - 210px)',
  },
  card: {
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    cursor: 'pointer',
    marginBottom: 10,
  },
  cardActive: {
    border: '1px solid rgba(99,102,241,0.8)',
    background: 'rgba(99,102,241,0.12)',
  },
  cardTitle: {
    fontWeight: 700,
    fontSize: 13,
    overflow: 'hidden',
    whiteSpace: 'nowrap',
    textOverflow: 'ellipsis',
  },
  cardSub: {
    fontSize: 12,
    opacity: 0.75,
    overflow: 'hidden',
    whiteSpace: 'nowrap',
    textOverflow: 'ellipsis',
    marginTop: 4,
  },
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
  },
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
  },
  labelRow: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 10,
  },
  label: {
    fontSize: 12,
    opacity: 0.75,
    fontWeight: 700,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
  },
  textarea: {
    width: '100%',
    height: 260,
    resize: 'vertical',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 10,
    boxSizing: 'border-box',
    outline: 'none',
  },
  input: {
    width: '100%',
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '8px 10px',
    outline: 'none',
    boxSizing: 'border-box',
  },
  select: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '8px 10px',
    outline: 'none',
  },
  btn: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.14)',
    background: 'rgba(255,255,255,0.04)',
    color: '#e5e7eb',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  btnPrimary: {
    borderRadius: 10,
    border: '1px solid rgba(99,102,241,0.6)',
    background: 'rgba(99,102,241,0.35)',
    color: '#fff',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  btnDanger: {
    borderRadius: 10,
    border: '1px solid rgba(248,113,113,0.55)',
    background: 'rgba(248,113,113,0.20)',
    color: '#fff',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  iconBtn: {
    border: 'none',
    background: 'transparent',
    color: '#cbd5e1',
    cursor: 'pointer',
  },
  smallBtn: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.14)',
    background: 'rgba(255,255,255,0.04)',
    color: '#e5e7eb',
    padding: '6px 8px',
    cursor: 'pointer',
    fontSize: 12,
  },
  empty: {
    opacity: 0.6,
    fontSize: 13,
    padding: 10,
  },
  emptyBox: {
    opacity: 0.65,
    fontSize: 12,
    padding: 12,
    borderRadius: 12,
    border: '1px dashed rgba(255,255,255,0.18)',
  },
  previewCard: {
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.14)',
  },
  mono: {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    fontSize: 12,
    opacity: 0.75,
    marginRight: 8,
  },
  miniMono: {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    fontSize: 11,
    opacity: 0.65,
  },
  modalMask: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0,0,0,0.55)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 60,
  },
  modal: {
    width: 820,
    maxHeight: '86vh',
    overflow: 'auto',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: '#0b1220',
    padding: 14,
  },
}



