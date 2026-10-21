import { useEffect, useMemo, useState } from 'react'
import api from '../api/client'
import type { PromptTemplateRead } from '../api/types'

type Draft = {
  title: string
  category: string
  prompt: string
}

const categoryOptions = [
  { value: 'storyboard', label: '分镜/分场' },
  { value: 'writing', label: '写作' },
  { value: 'misc', label: '其他' },
]

export function PromptsSettingsPage() {
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string>('')
  const [success, setSuccess] = useState<string>('')

  const [templates, setTemplates] = useState<PromptTemplateRead[]>([])
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState<string>('all')

  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const selected = useMemo(() => templates.find((t) => t.key === selectedKey) || null, [templates, selectedKey])

  const [draft, setDraft] = useState<Draft>({ title: '', category: 'misc', prompt: '' })
  const [saving, setSaving] = useState(false)
  const [showCreate, setShowCreate] = useState(false)
  const [createDraft, setCreateDraft] = useState<{ key: string; title: string; category: string; prompt: string }>({
    key: '',
    title: '',
    category: 'misc',
    prompt: '',
  })
  const [creating, setCreating] = useState(false)

  async function refresh() {
    setLoading(true)
    setError('')
    setSuccess('')
    try {
      const res = await api.getPromptTemplates()
      const list = res.data || []
      setTemplates(list)
      setSelectedKey((prev) => prev ?? (list[0]?.key || null))
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '加载失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    refresh().catch(() => {})
  }, [])

  useEffect(() => {
    if (!selected) return
    setDraft({
      title: selected.title || '',
      category: selected.category || 'misc',
      prompt: selected.prompt || '',
    })
  }, [selected?.key])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return (templates || [])
      .filter((t) => (category === 'all' ? true : t.category === category))
      .filter((t) => {
        if (!q) return true
        return (
          t.key.toLowerCase().includes(q) ||
          (t.title || '').toLowerCase().includes(q) ||
          (t.prompt || '').toLowerCase().includes(q)
        )
      })
      .sort((a, b) => {
        // 内置优先，再按分类/标题
        if (a.is_builtin !== b.is_builtin) return a.is_builtin ? -1 : 1
        if (a.category !== b.category) return a.category.localeCompare(b.category)
        return a.title.localeCompare(b.title)
      })
  }, [templates, query, category])

  async function save() {
    if (!selected) return
    setSaving(true)
    setError('')
    setSuccess('')
    try {
      const res = await api.upsertPromptTemplate(selected.key, {
        title: draft.title,
        category: draft.category,
        prompt: draft.prompt,
      })
      setTemplates((prev) => prev.map((t) => (t.key === selected.key ? res.data : t)))
      setSuccess('已保存')
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setSaving(false)
    }
  }

  async function resetToBuiltin() {
    if (!selected) return
    setSaving(true)
    setError('')
    setSuccess('')
    try {
      const res = await api.resetPromptTemplate(selected.key)
      setTemplates((prev) => prev.map((t) => (t.key === selected.key ? res.data : t)))
      setSuccess('已重置为内置默认')
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '重置失败')
    } finally {
      setSaving(false)
    }
  }

  async function deleteTemplate() {
    if (!selected) return
    if (!window.confirm(`确定删除模板「${selected.title}」？`)) return
    setSaving(true)
    setError('')
    setSuccess('')
    try {
      await api.deletePromptTemplate(selected.key)
      await refresh()
      setSuccess(selected.is_builtin ? '已重置' : '已删除')
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '删除失败')
    } finally {
      setSaving(false)
    }
  }

  async function create() {
    const key = createDraft.key.trim()
    if (!key) return
    setCreating(true)
    setError('')
    setSuccess('')
    try {
      const res = await api.createPromptTemplate({
        key,
        title: createDraft.title.trim() || key,
        category: createDraft.category,
        prompt: createDraft.prompt || '',
      })
      setTemplates((prev) => [...prev, res.data])
      setShowCreate(false)
      setCreateDraft({ key: '', title: '', category: 'misc', prompt: '' })
      setSelectedKey(res.data.key)
      setSuccess('已创建')
    } catch (e: any) {
      setError(e?.response?.data?.detail || e?.message || '创建失败')
    } finally {
      setCreating(false)
    }
  }

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <div>
          <h2 style={{ margin: 0, fontSize: 18 }}>Prompt 模板</h2>
          <div style={{ fontSize: 12, opacity: 0.7 }}>用于分场/分镜/写作等任务的内置与自定义 Prompt</div>
        </div>
        <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
          <button style={styles.btn} onClick={() => refresh().catch(() => {})} disabled={loading}>
            刷新
          </button>
          <button style={styles.btnPrimary} onClick={() => setShowCreate(true)}>
            + 新建模板
          </button>
        </div>
      </div>

      {error ? <div style={styles.error}>{error}</div> : null}
      {success ? <div style={styles.success}>{success}</div> : null}

      <div style={styles.body}>
        <aside style={styles.left}>
          <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 10 }}>
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索 key/标题/内容" style={styles.input} />
            <select value={category} onChange={(e) => setCategory(e.target.value)} style={styles.select}>
              <option value="all">全部</option>
              {categoryOptions.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </div>

          {loading ? <div style={{ opacity: 0.7 }}>加载中…</div> : null}
          {!loading && filtered.length === 0 ? <div style={{ opacity: 0.6 }}>无匹配模板</div> : null}

          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {filtered.map((t) => (
              <button
                key={t.key}
                onClick={() => setSelectedKey(t.key)}
                style={{
                  ...styles.item,
                  ...(t.key === selectedKey ? styles.itemActive : null),
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10 }}>
                  <div style={{ fontWeight: 900, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{t.title}</div>
                  <div style={{ fontSize: 11, opacity: 0.7 }}>{t.is_builtin ? (t.is_modified ? '内置(已改)' : '内置') : '自定义'}</div>
                </div>
                <div style={{ fontSize: 11, opacity: 0.7, marginTop: 4, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {t.key}
                </div>
              </button>
            ))}
          </div>
        </aside>

        <main style={styles.right}>
          {!selected ? (
            <div style={{ opacity: 0.7 }}>请选择一个模板</div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div style={styles.metaRow}>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  <div style={styles.smallLabel}>Key</div>
                  <div style={styles.monoBox}>{selected.key}</div>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  <div style={styles.smallLabel}>Category</div>
                  <select value={draft.category} onChange={(e) => setDraft((p) => ({ ...p, category: e.target.value }))} style={styles.select}>
                    {categoryOptions.map((c) => (
                      <option key={c.value} value={c.value}>
                        {c.label}
                      </option>
                    ))}
                    <option value="misc">其他</option>
                  </select>
                </div>
              </div>

              <div>
                <div style={styles.smallLabel}>标题</div>
                <input value={draft.title} onChange={(e) => setDraft((p) => ({ ...p, title: e.target.value }))} style={styles.input} />
              </div>

              <div>
                <div style={styles.smallLabel}>
                  Prompt
                  {selected.variables?.length ? (
                    <span style={{ marginLeft: 10, fontWeight: 700, opacity: 0.7 }}>
                      可用变量：{selected.variables.map((v) => `{${v}}`).join(' ')}
                    </span>
                  ) : null}
                </div>
                <textarea
                  value={draft.prompt}
                  onChange={(e) => setDraft((p) => ({ ...p, prompt: e.target.value }))}
                  style={styles.textarea}
                />
              </div>

              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10 }}>
                {selected.is_builtin ? (
                  <button style={styles.btn} onClick={() => resetToBuiltin().catch(() => {})} disabled={saving || !selected.is_modified}>
                    重置为内置
                  </button>
                ) : null}
                <button style={styles.btnDanger} onClick={() => deleteTemplate().catch(() => {})} disabled={saving}>
                  {selected.is_builtin ? '删除覆盖(重置)' : '删除'}
                </button>
                <button style={styles.btnPrimary} onClick={() => save().catch(() => {})} disabled={saving}>
                  {saving ? '保存中…' : '保存'}
                </button>
              </div>
            </div>
          )}
        </main>
      </div>

      {showCreate ? (
        <div style={styles.modalMask} onClick={() => !creating && setShowCreate(false)}>
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 18, fontWeight: 900, marginBottom: 12 }}>新建模板</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={styles.smallLabel}>key（唯一，建议用英文下划线）</div>
                <input value={createDraft.key} onChange={(e) => setCreateDraft((p) => ({ ...p, key: e.target.value }))} style={styles.input} />
              </div>
              <div>
                <div style={styles.smallLabel}>标题</div>
                <input value={createDraft.title} onChange={(e) => setCreateDraft((p) => ({ ...p, title: e.target.value }))} style={styles.input} />
              </div>
              <div>
                <div style={styles.smallLabel}>分类</div>
                <select value={createDraft.category} onChange={(e) => setCreateDraft((p) => ({ ...p, category: e.target.value }))} style={styles.select}>
                  {categoryOptions.map((c) => (
                    <option key={c.value} value={c.value}>
                      {c.label}
                    </option>
                  ))}
                  <option value="misc">其他</option>
                </select>
              </div>
              <div>
                <div style={styles.smallLabel}>Prompt</div>
                <textarea value={createDraft.prompt} onChange={(e) => setCreateDraft((p) => ({ ...p, prompt: e.target.value }))} style={{ ...styles.textarea, height: 180 }} />
              </div>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 14 }}>
              <button style={styles.btn} onClick={() => setShowCreate(false)} disabled={creating}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => create().catch(() => {})} disabled={creating || !createDraft.key.trim()}>
                {creating ? '创建中…' : '创建'}
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
  header: {
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.04)',
    display: 'flex',
    justifyContent: 'space-between',
    gap: 12,
    flexWrap: 'wrap',
    alignItems: 'center',
  },
  body: { display: 'grid', gridTemplateColumns: '360px 1fr', gap: 12, minHeight: 'calc(100vh - 160px)' },
  left: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'auto',
  },
  right: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'auto',
  },
  item: {
    textAlign: 'left',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 10,
    cursor: 'pointer',
  },
  itemActive: {
    border: '1px solid rgba(99,102,241,0.8)',
    background: 'rgba(99,102,241,0.12)',
  },
  smallLabel: { fontSize: 12, fontWeight: 900, opacity: 0.75, marginBottom: 6 },
  metaRow: { display: 'grid', gridTemplateColumns: '1fr 220px', gap: 12 },
  monoBox: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    padding: '8px 10px',
    fontSize: 12,
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
  },
  input: { width: '100%', borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '9px 10px', outline: 'none', boxSizing: 'border-box' },
  textarea: {
    width: '100%',
    height: 420,
    resize: 'vertical',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 10,
    boxSizing: 'border-box',
    outline: 'none',
  },
  select: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.10)', background: 'rgba(0,0,0,0.18)', color: '#e5e7eb', padding: '9px 10px', outline: 'none' },
  btn: { borderRadius: 10, border: '1px solid rgba(255,255,255,0.14)', background: 'rgba(255,255,255,0.04)', color: '#e5e7eb', padding: '8px 10px', cursor: 'pointer' },
  btnPrimary: { borderRadius: 10, border: '1px solid rgba(99,102,241,0.6)', background: 'rgba(99,102,241,0.35)', color: '#fff', padding: '8px 10px', cursor: 'pointer' },
  btnDanger: { borderRadius: 10, border: '1px solid rgba(248,113,113,0.55)', background: 'rgba(248,113,113,0.20)', color: '#fff', padding: '8px 10px', cursor: 'pointer' },
  error: { color: '#f87171', fontSize: 12 },
  success: { color: '#34d399', fontSize: 12 },
  modalMask: { position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60 },
  modal: { width: 560, borderRadius: 14, border: '1px solid rgba(255,255,255,0.10)', background: '#0b1220', padding: 16, maxHeight: '80vh', overflow: 'auto' },
}


