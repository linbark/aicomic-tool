export function PlaceholderPage({ title }: { title: string }) {
  return (
    <div style={{ padding: 12 }}>
      <h2 style={{ margin: '0 0 10px 0' }}>{title}</h2>
      <p style={{ opacity: 0.85, margin: 0 }}>该页面将在 React 迁移过程中逐步补齐。</p>
    </div>
  )
}


