import { memo } from 'react'
import { cardStyle, transitions } from '../../styles/shared'

type CardProps = {
  children: React.ReactNode
  onClick?: () => void
  active?: boolean
  style?: React.CSSProperties
}

export const Card = memo(function Card({ children, onClick, active, style }: CardProps) {
  return (
    <div
      onClick={onClick}
      style={{
        ...cardStyle,
        cursor: onClick ? 'pointer' : 'default',
        border: active ? '1px solid rgba(99,102,241,0.7)' : cardStyle.border,
        background: active ? 'rgba(99,102,241,0.12)' : cardStyle.background,
        transition: `all ${transitions.normal} ease`,
        ...style,
      }}
      onMouseEnter={(e) => {
        if (onClick) {
          e.currentTarget.style.borderColor = active ? 'rgba(99,102,241,0.8)' : 'rgba(255,255,255,0.15)'
          e.currentTarget.style.background = active ? 'rgba(99,102,241,0.15)' : 'rgba(0,0,0,0.20)'
        }
      }}
      onMouseLeave={(e) => {
        if (onClick) {
          e.currentTarget.style.borderColor = active ? 'rgba(99,102,241,0.7)' : (cardStyle.border as string)
          e.currentTarget.style.background = active ? 'rgba(99,102,241,0.12)' : (cardStyle.background as string)
        }
      }}
      onFocus={(e) => {
        if (onClick) {
          e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.6)'
          e.currentTarget.style.outlineOffset = '2px'
        }
      }}
      onBlur={(e) => {
        e.currentTarget.style.outline = 'none'
      }}
      tabIndex={onClick ? 0 : undefined}
      role={onClick ? 'button' : undefined}
      onKeyDown={(e) => {
        if (onClick && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault()
          onClick()
        }
      }}
    >
      {children}
    </div>
  )
})
