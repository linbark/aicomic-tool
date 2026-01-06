import { useEffect, useState } from 'react'
import api from '../api/client'
import type { AiSettingsRead } from '../api/types'

type FormState = Omit<AiSettingsRead, 'has_api_key'>

export function AiSettingsPage() {
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string>('')
  const [success, setSuccess] = useState<string>('')
  const [isSaving, setIsSaving] = useState(false)
  const [isTesting, setIsTesting] = useState(false)

  const [hasApiKey, setHasApiKey] = useState(false)
  const [apiKeyInput, setApiKeyInput] = useState('')
  const [clearApiKey, setClearApiKey] = useState(false)

  const [form, setForm] = useState<FormState>({
    base_url: 'https://api.deepseek.com',
    model: 'deepseek-chat',
    temperature: 0.2,
    max_tokens: 8192,
    timeout_seconds: 30,
  })

  async function load() {
    setLoading(true)
    setError('')
    setSuccess('')
    try {
      const { data } = await api.getAiSettings()
      setHasApiKey(!!data?.has_api_key)
      setForm({
        base_url: data?.base_url || 'https://api.deepseek.com',
        model: data?.model || 'deepseek-chat',
        temperature: Number(data?.temperature ?? 0.2),
        max_tokens: Number(data?.max_tokens ?? 8192),
        timeout_seconds: Number(data?.timeout_seconds ?? 30),
      })
    } catch (e: any) {
      setError('加载失败：' + (e?.response?.data?.detail || e?.message || '未知错误'))
    } finally {
      setLoading(false)
    }
  }

  async function handleSave() {
    if (isSaving) return
    setIsSaving(true)
    setError('')
    setSuccess('')
    try {
      const payload: any = {
        base_url: form.base_url,
        model: form.model,
        temperature: form.temperature,
        max_tokens: form.max_tokens,
        timeout_seconds: form.timeout_seconds,
      }
      if (clearApiKey) payload.api_key = ''
      else if (apiKeyInput.trim()) payload.api_key = apiKeyInput.trim()

      const { data } = await api.updateAiSettings(payload)
      setHasApiKey(!!data?.has_api_key)
      setApiKeyInput('')
      setClearApiKey(false)
      setSuccess('保存成功')
    } catch (e: any) {
      setError('保存失败：' + (e?.response?.data?.detail || e?.message || '未知错误'))
    } finally {
      setIsSaving(false)
    }
  }

  async function handleTest() {
    if (isTesting) return
    setIsTesting(true)
    setError('')
    setSuccess('')
    try {
      const { data } = await api.testAi()
      if (data?.ok) setSuccess(data?.detail || '连接成功')
      else setError(data?.detail || '连接失败')
    } catch (e: any) {
      setError('测试失败：' + (e?.response?.data?.detail || e?.message || '未知错误'))
    } finally {
      setIsTesting(false)
    }
  }

  useEffect(() => {
    load().catch(() => {})
  }, [])

  return (
    <div style={styles.wrap}>
      <div style={styles.header}>
        <div>
          <h2 style={{ margin: 0, fontSize: 18 }}>AI 配置（DeepSeek）</h2>
          <p style={{ margin: '6px 0 0 0', opacity: 0.7, fontSize: 12 }}>
            配置保存在本机应用数据目录；前端不会直接请求 DeepSeek，统一走后端代理。
          </p>
        </div>
        <div style={{ display: 'flex', gap: 10 }}>
          <button onClick={() => handleTest().catch(() => {})} disabled={isTesting} style={styles.btn}>
            {isTesting ? '测试中…' : '测试连接'}
          </button>
          <button onClick={() => handleSave().catch(() => {})} disabled={isSaving} style={styles.btnPrimary}>
            {isSaving ? '保存中…' : '保存配置'}
          </button>
        </div>
      </div>

      {loading ? <div style={{ opacity: 0.7 }}>加载中…</div> : null}

      {!loading ? (
        <div style={styles.card}>
          {error ? <div style={{ color: '#f87171', fontSize: 12 }}>{error}</div> : null}
          {success ? <div style={{ color: '#34d399', fontSize: 12 }}>{success}</div> : null}

          <div style={styles.grid2}>
            <Field label="Base URL">
              <input
                value={form.base_url}
                onChange={(e) => setForm((p) => ({ ...p, base_url: e.target.value }))}
                style={styles.input}
                placeholder="https://api.deepseek.com"
              />
            </Field>
            <Field label="Model">
              <input
                value={form.model}
                onChange={(e) => setForm((p) => ({ ...p, model: e.target.value }))}
                style={styles.input}
                placeholder="deepseek-chat"
              />
            </Field>
          </div>

          <div style={styles.grid3}>
            <Field label="temperature">
              <input
                value={String(form.temperature)}
                type="number"
                step="0.1"
                min={0}
                max={2}
                onChange={(e) => setForm((p) => ({ ...p, temperature: Number(e.target.value) }))}
                style={styles.input}
              />
            </Field>
            <Field label="max_tokens">
              <input
                value={String(form.max_tokens)}
                type="number"
                step="1"
                min={64}
                max={8192}
                onChange={(e) => setForm((p) => ({ ...p, max_tokens: Number(e.target.value) }))}
                style={styles.input}
              />
            </Field>
            <Field label="timeout(s)">
              <input
                value={String(form.timeout_seconds)}
                type="number"
                step="1"
                min={5}
                max={120}
                onChange={(e) => setForm((p) => ({ ...p, timeout_seconds: Number(e.target.value) }))}
                style={styles.input}
              />
            </Field>
          </div>

          <div style={{ marginTop: 4 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={styles.label}>API Key</div>
              <div style={{ fontSize: 11, color: hasApiKey ? '#34d399' : 'rgba(229,231,235,0.55)' }}>
                {hasApiKey ? '已设置' : '未设置'}
              </div>
            </div>
            <input
              value={apiKeyInput}
              onChange={(e) => setApiKeyInput(e.target.value)}
              type="password"
              style={styles.input}
              placeholder="输入新的 API Key（留空表示不修改）"
            />
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, marginTop: 10, fontSize: 12, opacity: 0.8 }}>
              <input type="checkbox" checked={clearApiKey} onChange={(e) => setClearApiKey(e.target.checked)} />
              清空 API Key
            </label>
          </div>
        </div>
      ) : null}
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={styles.label}>{label}</div>
      {children}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  wrap: {
    maxWidth: 860,
    margin: '0 auto',
    padding: 8,
  },
  header: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    gap: 12,
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.04)',
    marginBottom: 12,
  },
  card: {
    padding: 16,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.03)',
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  grid2: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: 12,
  },
  grid3: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr 1fr',
    gap: 12,
  },
  label: {
    fontSize: 12,
    opacity: 0.75,
    fontWeight: 700,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
    marginBottom: 6,
  },
  input: {
    width: '100%',
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '9px 10px',
    outline: 'none',
    boxSizing: 'border-box',
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
}


