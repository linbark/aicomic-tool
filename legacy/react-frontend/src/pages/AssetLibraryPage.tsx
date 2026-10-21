import { useEffect, useMemo, useState } from 'react'
import api, { getFileUrl } from '../api/client'
import type { AssetItemRead, AssetRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

type CategoryOption = { value: string; label: string }

const categoryOptions: CategoryOption[] = [
  { value: 'persona_visual', label: '角色（视觉）' },
  { value: 'persona_voice', label: '角色（声音）' },
  { value: 'background', label: '背景/场景' },
  { value: 'element', label: '场景元素/贴片' },
  { value: 'prop', label: '道具/物件' },
  { value: 'pose', label: '动作/镜头/分镜模板' },
  { value: 'vfx', label: '特效/转场' },
  { value: 'layout', label: '字幕/版式/字体' },
  { value: 'audio_music', label: '音频-音乐' },
  { value: 'audio_sfx', label: '音频-音效/环境' },
  { value: 'branding', label: '成片物料' },
  { value: 'ai_preset', label: 'AI 预设与工程模板' },
]

type EditForm = {
  id?: number
  name: string
  description: string
  base_prompt: string
  category: string
}

export function AssetLibraryPage() {
  const { projects, projectId, setProjectId } = useProjectSelection()
  const [selectedCategory, setSelectedCategory] = useState('persona_visual')
  const [items, setItems] = useState<AssetItemRead[]>([])
  const [loading, setLoading] = useState(false)

  const [showModal, setShowModal] = useState(false)
  const [isEditing, setIsEditing] = useState(false)
  const [form, setForm] = useState<EditForm>({
    name: '',
    description: '',
    base_prompt: '',
    category: 'persona_visual',
  })

  const [lightboxAsset, setLightboxAsset] = useState<AssetRead | null>(null)

  async function fetchItems(pid = projectId, category = selectedCategory) {
    if (!pid) {
      setItems([])
      return
    }
    setLoading(true)
    try {
      const res = await api.getAssetItems(pid, category)
      setItems(res.data || [])
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchItems().catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, selectedCategory])

  const empty = useMemo(() => !loading && (items?.length || 0) === 0, [loading, items])

  function openEditModal(item: AssetItemRead | null) {
    if (!projectId) {
      window.alert('请先选择项目')
      return
    }
    setIsEditing(!!item)
    setForm(
      item
        ? {
            id: item.id,
            name: item.name || '',
            description: item.description || '',
            base_prompt: item.base_prompt || '',
            category: item.category || selectedCategory,
          }
        : { name: '', description: '', base_prompt: '', category: selectedCategory },
    )
    setShowModal(true)
  }

  async function submit() {
    if (!projectId) return
    if (!form.name.trim()) return
    if (isEditing && form.id) {
      await api.updateAssetItem(form.id, {
        name: form.name,
        description: form.description,
        base_prompt: form.base_prompt,
        category: form.category,
      })
    } else {
      await api.createAssetItem(projectId, {
        name: form.name,
        description: form.description,
        base_prompt: form.base_prompt,
        category: form.category,
      })
    }
    setShowModal(false)
    await fetchItems(projectId, selectedCategory)
  }

  async function deleteItem(item: AssetItemRead) {
    if (!window.confirm('确定删除该资产条目？（将删除该条目及其所有文件）')) return
    await api.deleteAssetItem(item.id)
    await fetchItems(projectId, selectedCategory)
  }

  async function deleteAsset(asset: AssetRead) {
    if (!window.confirm('确定删除该素材文件？')) return
    await api.deleteProjectAsset(asset.id)
    await fetchItems(projectId, selectedCategory)
  }

  async function deleteAssetFromLightbox(asset: AssetRead) {
    await deleteAsset(asset)
    setLightboxAsset(null)
  }

  async function handleUpload(itemId: number, files: FileList | null) {
    if (!files || files.length === 0) return
    const list = Array.from(files)
    for (const f of list) {
      const fd = new FormData()
      fd.append('file', f)
      await api.uploadAssetItemAsset(itemId, fd)
    }
    await fetchItems(projectId, selectedCategory)
  }

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <div>
          <h2 style={{ margin: 0, fontSize: 20 }}>资产库</h2>
          <div style={{ fontSize: 12, opacity: 0.7 }}>Project Assets Library</div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
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
          <button onClick={() => openEditModal(null)} style={styles.btnPrimary}>
            + 新建资产条目
          </button>
        </div>
      </div>

      <div style={styles.toolbar}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={styles.label}>Category</div>
          <select value={selectedCategory} onChange={(e) => setSelectedCategory(e.target.value)} style={styles.select}>
            {categoryOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div style={styles.body}>
        {loading ? <div style={{ opacity: 0.7 }}>加载中…</div> : null}
        {empty ? <div style={styles.empty}>暂无资产条目，请点击右上角创建</div> : null}

        {items.map((item) => (
          <div key={item.id} style={styles.itemCard}>
            <div style={styles.itemLeft}>
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10 }}>
                <div>
                  <div style={{ fontSize: 18, fontWeight: 900 }}>{item.name}</div>
                  <div style={{ fontSize: 12, opacity: 0.7 }}>Asset Count: {(item.assets || []).length}</div>
                </div>
                <div style={{ display: 'flex', gap: 8 }}>
                  <button onClick={() => openEditModal(item)} style={styles.iconBtn} title="编辑">
                    编辑
                  </button>
                  <button onClick={() => deleteItem(item).catch(() => {})} style={styles.iconBtnDanger} title="删除（含文件）">
                    删除
                  </button>
                </div>
              </div>

              <div style={{ marginTop: 12, display: 'flex', flexDirection: 'column', gap: 10 }}>
                <div>
                  <div style={styles.smallLabel}>Description</div>
                  <div style={styles.descBox}>{item.description || '暂无描述…'}</div>
                </div>

                {selectedCategory === 'persona_visual' ? (
                  <div>
                    <div style={styles.smallLabel}>Base Prompt</div>
                    <div style={styles.promptBox}>{item.base_prompt || 'N/A'}</div>
                  </div>
                ) : null}
              </div>

              <div style={{ marginTop: 14, paddingTop: 14, borderTop: '1px solid rgba(255,255,255,0.08)' }}>
                <label style={styles.uploadLabel}>
                  <span>📤 上传素材 (图/视/音/文)</span>
                  <input
                    type="file"
                    multiple
                    accept="image/*,video/*,audio/*,.txt,.md,.pdf,.doc,.docx"
                    style={{ display: 'none' }}
                    onChange={(e) => handleUpload(item.id, e.target.files).catch(() => {})}
                  />
                </label>
              </div>
            </div>

            <div style={styles.itemRight}>
              {!item.assets || item.assets.length === 0 ? (
                <div style={styles.rightEmpty}>暂无素材，请从左侧上传</div>
              ) : (
                <div style={styles.assetRow}>
                  {(item.assets || []).map((asset) => (
                    <div key={asset.id} style={styles.assetThumb} onClick={() => setLightboxAsset(asset)}>
                      {asset.file_type === 'image' ? (
                        <img src={getFileUrl(asset.file_path)} style={styles.assetImg} />
                      ) : asset.file_type === 'video' ? (
                        <div style={{ width: '100%', height: '100%', position: 'relative' }}>
                          <video src={getFileUrl(asset.file_path)} style={styles.assetImg} muted preload="metadata" />
                          <div style={styles.playOverlay}>▶</div>
                        </div>
                      ) : asset.file_type === 'audio' ? (
                        <div style={styles.assetOther}>
                          <div style={{ fontSize: 34 }}>🔊</div>
                          <div style={styles.assetFilename}>{asset.file_path.split('/').pop()}</div>
                        </div>
                      ) : (
                        <div style={styles.assetOther}>
                          <div style={{ fontSize: 34 }}>📄</div>
                          <div style={styles.assetFilename}>{asset.file_path.split('/').pop()}</div>
                        </div>
                      )}

                      <button
                        onClick={(e) => {
                          e.stopPropagation()
                          deleteAsset(asset).catch(() => {})
                        }}
                        style={styles.thumbDelete}
                        title="删除文件"
                      >
                        删除
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      {/* 编辑/新建弹窗 */}
      {showModal ? (
        <div style={styles.modalMask}>
          <div style={styles.modal}>
            <div style={{ fontSize: 18, fontWeight: 900, marginBottom: 12 }}>{isEditing ? '编辑资产条目' : '新建资产条目'}</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <Field label="名称">
                <input value={form.name} onChange={(e) => setForm((p) => ({ ...p, name: e.target.value }))} style={styles.input} />
              </Field>
              <Field label="描述">
                <textarea value={form.description} onChange={(e) => setForm((p) => ({ ...p, description: e.target.value }))} style={{ ...styles.textarea, height: 96 }} />
              </Field>
              {form.category === 'persona_visual' ? (
                <Field label="Base Prompt">
                  <textarea value={form.base_prompt} onChange={(e) => setForm((p) => ({ ...p, base_prompt: e.target.value }))} style={{ ...styles.textarea, height: 96, fontFamily: styles.mono.fontFamily }} />
                </Field>
              ) : null}
              <Field label="分类">
                <select value={form.category} onChange={(e) => setForm((p) => ({ ...p, category: e.target.value }))} style={styles.select}>
                  {categoryOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 14 }}>
              <button onClick={() => setShowModal(false)} style={styles.btn}>
                取消
              </button>
              <button onClick={() => submit().catch(() => {})} style={styles.btnPrimary}>
                确定
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* Lightbox */}
      {lightboxAsset ? (
        <div style={styles.lightboxMask} onClick={() => setLightboxAsset(null)}>
          <div style={styles.lightbox} onClick={(e) => e.stopPropagation()}>
            <div style={styles.lightboxTop}>
              <button onClick={() => deleteAssetFromLightbox(lightboxAsset).catch(() => {})} style={styles.btnDanger}>
                删除
              </button>
              <button onClick={() => setLightboxAsset(null)} style={styles.iconBtn}>
                ✕
              </button>
            </div>

            {lightboxAsset.file_type === 'image' ? (
              <img src={getFileUrl(lightboxAsset.file_path)} style={styles.lightboxMedia} />
            ) : lightboxAsset.file_type === 'video' ? (
              <video src={getFileUrl(lightboxAsset.file_path)} controls autoPlay style={styles.lightboxMedia} />
            ) : lightboxAsset.file_type === 'audio' ? (
              <audio src={getFileUrl(lightboxAsset.file_path)} controls style={{ width: 600 }} />
            ) : (
              <div style={styles.docBox}>
                <div style={{ fontSize: 48, marginBottom: 10 }}>📄</div>
                <div style={{ marginBottom: 12, opacity: 0.85 }}>文档文件无法直接预览</div>
                <a href={getFileUrl(lightboxAsset.file_path)} target="_blank" rel="noreferrer" style={styles.btnPrimary}>
                  下载/在新标签页打开
                </a>
              </div>
            )}

            <div style={{ marginTop: 10, fontSize: 12, opacity: 0.7, textAlign: 'center', ...styles.mono }}>
              {lightboxAsset.file_path}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={styles.smallLabel}>{label}</div>
      {children}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: { display: 'flex', flexDirection: 'column', height: 'calc(100vh - 32px)' },
  header: {
    padding: 14,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.04)',
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
    flexWrap: 'wrap',
  },
  toolbar: { padding: '10px 4px 0 4px' },
  label: { fontSize: 12, fontWeight: 900, opacity: 0.7, textTransform: 'uppercase', letterSpacing: 0.5 },
  smallLabel: { fontSize: 12, fontWeight: 900, opacity: 0.7, marginBottom: 6 },
  body: { padding: 14, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 14 },
  empty: {
    padding: 32,
    borderRadius: 12,
    border: '1px dashed rgba(255,255,255,0.15)',
    opacity: 0.75,
    textAlign: 'center',
  },
  itemCard: {
    borderRadius: 14,
    border: '1px solid rgba(255,255,255,0.08)',
    overflow: 'hidden',
    display: 'flex',
    minHeight: 200,
    background: 'rgba(255,255,255,0.02)',
  },
  itemLeft: {
    width: 360,
    minWidth: 320,
    padding: 14,
    borderRight: '1px solid rgba(255,255,255,0.06)',
    background: 'rgba(0,0,0,0.12)',
    boxSizing: 'border-box',
  },
  itemRight: { flex: 1, minWidth: 0, padding: 12, boxSizing: 'border-box' },
  rightEmpty: { height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', opacity: 0.55, fontSize: 12 },
  descBox: { padding: 10, borderRadius: 12, border: '1px solid rgba(255,255,255,0.08)', background: 'rgba(0,0,0,0.10)', fontSize: 12, opacity: 0.9 },
  promptBox: { padding: 10, borderRadius: 12, border: '1px solid rgba(99,102,241,0.25)', background: 'rgba(99,102,241,0.10)', fontSize: 12, ...({ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace' } as any) },
  uploadLabel: {
    display: 'flex',
    justifyContent: 'center',
    alignItems: 'center',
    gap: 8,
    padding: '10px 12px',
    borderRadius: 12,
    border: '1px dashed rgba(255,255,255,0.20)',
    cursor: 'pointer',
    background: 'rgba(255,255,255,0.02)',
    fontSize: 12,
    opacity: 0.9,
  },
  assetRow: { display: 'flex', gap: 12, overflowX: 'auto', paddingBottom: 6, alignItems: 'center' },
  assetThumb: {
    width: 144,
    height: 192,
    flex: '0 0 auto',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    overflow: 'hidden',
    background: 'rgba(255,255,255,0.03)',
    position: 'relative',
    cursor: 'pointer',
  },
  assetImg: { width: '100%', height: '100%', objectFit: 'cover' },
  assetOther: { width: '100%', height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 10, padding: 10, boxSizing: 'border-box', opacity: 0.85 },
  assetFilename: { fontSize: 11, opacity: 0.75, textAlign: 'center', wordBreak: 'break-all', maxHeight: 40, overflow: 'hidden' },
  playOverlay: { position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#fff', fontWeight: 900, background: 'rgba(0,0,0,0.18)' },
  thumbDelete: {
    position: 'absolute',
    top: 8,
    right: 8,
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.12)',
    background: 'rgba(0,0,0,0.35)',
    color: '#fff',
    padding: '6px 8px',
    fontSize: 12,
    cursor: 'pointer',
  },
  iconBtn: { border: '1px solid rgba(255,255,255,0.14)', background: 'rgba(255,255,255,0.04)', color: '#e5e7eb', padding: '8px 10px', borderRadius: 10, cursor: 'pointer' },
  iconBtnDanger: { border: '1px solid rgba(248,113,113,0.45)', background: 'rgba(248,113,113,0.18)', color: '#fff', padding: '8px 10px', borderRadius: 10, cursor: 'pointer' },
  btn: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.14)', background: 'rgba(255,255,255,0.04)', color: '#e5e7eb', padding: '8px 10px', cursor: 'pointer' },
  btnPrimary: { borderRadius: 10, border: '1px solid rgba(99,102,241,0.6)', background: 'rgba(99,102,241,0.35)', color: '#fff', padding: '8px 10px', cursor: 'pointer' },
  btnDanger: { borderRadius: 10, border: '1px solid rgba(248,113,113,0.55)', background: 'rgba(248,113,113,0.20)', color: '#fff', padding: '8px 10px', cursor: 'pointer' },
  select: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '8px 10px', outline: 'none' },
  input: { width: '100%', borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '9px 10px', outline: 'none', boxSizing: 'border-box' },
  textarea: { width: '100%', borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '9px 10px', outline: 'none', boxSizing: 'border-box', resize: 'vertical' },
  modalMask: { position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 50 },
  modal: { width: 420, borderRadius: 14, border: '1px solid rgba(255,255,255,0.10)', background: '#0b1220', padding: 16 },
  lightboxMask: { position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.88)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60 },
  lightbox: { maxWidth: '90vw', maxHeight: '90vh', padding: 14, position: 'relative' },
  lightboxTop: { display: 'flex', justifyContent: 'flex-end', gap: 10, marginBottom: 10 },
  lightboxMedia: { maxWidth: '90vw', maxHeight: '78vh', borderRadius: 12, boxShadow: '0 16px 30px rgba(0,0,0,0.45)' },
  docBox: { background: 'rgba(255,255,255,0.06)', border: '1px solid rgba(255,255,255,0.10)', borderRadius: 14, padding: 20, textAlign: 'center', width: 420 },
  mono: { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace' },
}


