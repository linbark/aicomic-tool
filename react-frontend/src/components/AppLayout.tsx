import { NavLink } from 'react-router-dom'

type Props = {
  children: React.ReactNode
}

export function AppLayout({ children }: Props) {
  return (
    <div style={styles.shell}>
      <aside style={styles.sidebar}>
        <div style={styles.brand}>AIComicTool</div>
        <nav style={styles.nav}>
          <NavItem to="/script" label="剧本" />
          <NavItem to="/events" label="事件" />
          <NavItem to="/assets" label="资产" />
          <NavItem to="/settings/ai" label="AI 设置" />
          <NavItem to="/settings/prompts" label="Prompt 模板" />
        </nav>
      </aside>
      <main style={styles.main}>{children}</main>
    </div>
  )
}

function NavItem({ to, label }: { to: string; label: string }) {
  return (
    <NavLink
      to={to}
      style={({ isActive }) => ({
        ...styles.navItem,
        ...(isActive ? styles.navItemActive : null),
      })}
    >
      {label}
    </NavLink>
  )
}

const styles: Record<string, React.CSSProperties> = {
  shell: {
    display: 'flex',
    height: '100vh',
    width: '100vw',
    overflow: 'hidden',
    background: '#0b1220',
    color: '#e5e7eb',
    fontFamily:
      'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Arial, "Apple Color Emoji", "Segoe UI Emoji"',
  },
  sidebar: {
    width: 220,
    borderRight: '1px solid rgba(255,255,255,0.08)',
    padding: 14,
    boxSizing: 'border-box',
  },
  brand: {
    fontWeight: 700,
    marginBottom: 12,
    letterSpacing: 0.2,
  },
  nav: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  navItem: {
    padding: '10px 10px',
    borderRadius: 10,
    textDecoration: 'none',
    color: '#e5e7eb',
    background: 'transparent',
  },
  navItemActive: {
    background: 'rgba(99,102,241,0.25)',
    border: '1px solid rgba(99,102,241,0.35)',
  },
  main: {
    flex: 1,
    overflow: 'auto',
    padding: 16,
    boxSizing: 'border-box',
  },
}


