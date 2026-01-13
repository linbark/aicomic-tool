import { useEffect, useState, memo, useCallback } from 'react'
import api from '../api/client'
import type { AiSettingsRead } from '../api/types'
import { Button } from '../components/ui/Button'
import { Input } from '../components/ui/Input'
import { panelStyle } from '../styles/shared'

type FormState = Omit<AiSettingsRead, 'has_api_key'>

export const AiSettingsPage = memo(function AiSettingsPage() {
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
        // 如果 max_tokens 为 null/undefined，显示为 0（表示不限制）
        max_tokens: data?.max_tokens === null || data?.max_tokens === undefined ? 0 : Number(data.max_tokens),
        timeout_seconds: Number(data?.timeout_seconds ?? 30),
      })
    } catch (e: any) {
      setError('加载失败：' + (e?.response?.data?.detail || e?.message || '未知错误'))
    } finally {
      setLoading(false)
    }
  }

  const handleSave = useCallback(async () => {
    if (isSaving) return
    setIsSaving(true)
    setError('')
    setSuccess('')
    try {
      const payload: any = {
        base_url: form.base_url,
        model: form.model,
        temperature: form.temperature,
        // 如果 max_tokens 为 0，发送 0（后端会将其转换为 None，表示不限制）
        max_tokens: form.max_tokens === 0 ? 0 : form.max_tokens,
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
  }, [isSaving, form, apiKeyInput, clearApiKey])

  const handleTest = useCallback(async () => {
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
  }, [isTesting])

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
          <Button onClick={() => handleTest().catch(() => {})} disabled={isTesting}>
            {isTesting ? '测试中…' : '测试连接'}
          </Button>
          <Button variant="primary" onClick={() => handleSave().catch(() => {})} disabled={isSaving}>
            {isSaving ? '保存中…' : '保存配置'}
          </Button>
        </div>
      </div>

      {loading ? <div style={{ opacity: 0.7 }}>加载中…</div> : null}

      {!loading ? (
        <div style={panelStyle}>
          {error ? (
            <div style={{ color: '#f87171', fontSize: 12 }} role="alert" aria-live="assertive">
              {error}
            </div>
          ) : null}
          {success ? (
            <div style={{ color: '#34d399', fontSize: 12 }} role="status" aria-live="polite">
              {success}
            </div>
          ) : null}

          <div style={styles.grid2}>
            <Field label="Base URL">
              <Input
                value={form.base_url}
                onChange={(e) => setForm((p) => ({ ...p, base_url: e.target.value }))}
                placeholder="https://api.deepseek.com"
                aria-label="Base URL"
              />
            </Field>
            <Field label="Model">
              <Input
                value={form.model}
                onChange={(e) => setForm((p) => ({ ...p, model: e.target.value }))}
                placeholder="deepseek-chat"
                aria-label="Model"
              />
            </Field>
          </div>

          <div style={styles.grid3}>
            <Field label="temperature">
              <Input
                value={String(form.temperature)}
                type="number"
                step="0.1"
                min={0}
                max={2}
                onChange={(e) => setForm((p) => ({ ...p, temperature: Number(e.target.value) }))}
                aria-label="Temperature"
              />
            </Field>
            <Field label="max_tokens (0=不限制)">
              <Input
                value={String(form.max_tokens)}
                type="number"
                step="1"
                min={0}
                placeholder="0 表示不限制"
                onChange={(e) => {
                  const val = e.target.value === '' ? 0 : Number(e.target.value)
                  setForm((p) => ({ ...p, max_tokens: val < 0 ? 0 : val }))
                }}
                aria-label="Max tokens (0 means unlimited)"
              />
            </Field>
            <Field label="timeout(s)">
              <Input
                value={String(form.timeout_seconds)}
                type="number"
                step="1"
                min={5}
                max={120}
                onChange={(e) => setForm((p) => ({ ...p, timeout_seconds: Number(e.target.value) }))}
                aria-label="Timeout seconds"
              />
            </Field>
          </div>

          <div style={{ marginTop: 4 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={styles.label}>API Key</div>
              <div style={{ fontSize: 11, color: hasApiKey ? '#34d399' : 'rgba(229,231,235,0.55)' }} role="status" aria-live="polite">
                {hasApiKey ? '已设置' : '未设置'}
              </div>
            </div>
            <Input
              value={apiKeyInput}
              onChange={(e) => setApiKeyInput(e.target.value)}
              type="password"
              placeholder="输入新的 API Key（留空表示不修改）"
              aria-label="API Key"
            />
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, marginTop: 10, fontSize: 12, opacity: 0.8, cursor: 'pointer' }}>
              <input type="checkbox" checked={clearApiKey} onChange={(e) => setClearApiKey(e.target.checked)} />
              清空 API Key
            </label>
          </div>
        </div>
      ) : null}
    </div>
  )
})

const Field = memo(function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={styles.label}>{label}</div>
      {children}
    </div>
  )
})

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
}


