import { NavLink } from 'react-router-dom'
import { memo } from 'react'

type Props = {
  children: React.ReactNode
}

export const AppLayout = memo(function AppLayout({ children }: Props) {
  return (
    <div style={styles.shell}>
      <header style={styles.header} role="banner" aria-label="主导航">
        <div style={styles.brand}>星晴</div>
        <nav style={styles.nav}>
          <NavItem to="/script" label="剧本" />
          <NavItem to="/events" label="事件" />
          <NavItem to="/assets" label="资产" />
          <NavItem to="/settings/ai" label="AI 设置" />
          <NavItem to="/settings/prompts" label="Prompt 模板" />
        </nav>
      </header>
      <main style={styles.main} role="main">{children}</main>
    </div>
  )
})

const NavItem = memo(function NavItem({ to, label }: { to: string; label: string }) {
  return (
    <NavLink
      to={to}
      style={({ isActive }) => ({
        ...styles.navItem,
        ...(isActive ? styles.navItemActive : null),
      })}
      onMouseEnter={(e) => {
        if (!e.currentTarget.classList.contains('active')) {
          e.currentTarget.style.background = 'rgba(255,255,255,0.06)'
        }
      }}
      onMouseLeave={(e) => {
        if (!e.currentTarget.classList.contains('active')) {
          e.currentTarget.style.background = 'transparent'
        }
      }}
      onFocus={(e) => {
        e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.6)'
        e.currentTarget.style.outlineOffset = '2px'
      }}
      onBlur={(e) => {
        e.currentTarget.style.outline = 'none'
      }}
    >
      {label}
    </NavLink>
  )
})

const styles: Record<string, React.CSSProperties> = {
  shell: {
    display: 'flex',
    flexDirection: 'column',
    height: '100vh',
    width: '100vw',
    overflow: 'hidden',
    background: '#0b1220',
    color: '#e5e7eb',
    fontFamily:
      'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Arial, "Apple Color Emoji", "Segoe UI Emoji"',
  },
  header: {
    height: 56,
    borderBottom: '1px solid rgba(255,255,255,0.08)',
    padding: '0 20px',
    boxSizing: 'border-box',
    display: 'flex',
    alignItems: 'center',
    gap: 32,
    background: 'rgba(11, 18, 32, 0.95)',
  },
  brand: {
    fontWeight: 700,
    fontSize: 18,
    letterSpacing: 0.5,
    color: '#fff',
  },
  nav: {
    display: 'flex',
    flexDirection: 'row',
    gap: 8,
    height: '100%',
    alignItems: 'center',
  },
  navItem: {
    padding: '6px 14px',
    borderRadius: 8,
    textDecoration: 'none',
    color: 'rgba(255,255,255,0.75)',
    background: 'transparent',
    cursor: 'pointer',
    transition: 'all 200ms ease',
    display: 'block',
    fontSize: 14,
    fontWeight: 500,
  },
  navItemActive: {
    background: 'rgba(99,102,241,0.2)',
    border: '1px solid rgba(99,102,241,0.3)',
    color: '#fff',
  },
  main: {
    flex: 1,
    overflow: 'auto',
    padding: 16,
    boxSizing: 'border-box',
    position: 'relative',
  },
}


