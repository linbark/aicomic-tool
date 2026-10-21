import { memo } from 'react'
import { baseButtonStyle, primaryButtonStyle, transitions } from '../../styles/shared'

type ButtonProps = React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'default' | 'primary'
  children: React.ReactNode
}

export const Button = memo(function Button({ variant = 'default', children, style, disabled, ...props }: ButtonProps) {
  const baseStyle = variant === 'primary' ? primaryButtonStyle : baseButtonStyle
  
  return (
    <button
      {...props}
      disabled={disabled}
      style={{
        ...baseStyle,
        opacity: disabled ? 0.5 : 1,
        cursor: disabled ? 'not-allowed' : 'pointer',
        transition: `all ${transitions.normal} ease`,
        ...style,
      }}
      onMouseEnter={(e) => {
        if (!disabled) {
          e.currentTarget.style.borderColor = variant === 'primary' ? 'rgba(99,102,241,0.8)' : 'rgba(255,255,255,0.2)'
          e.currentTarget.style.background = variant === 'primary' ? 'rgba(99,102,241,0.45)' : 'rgba(255,255,255,0.08)'
        }
      }}
      onMouseLeave={(e) => {
        if (!disabled) {
          e.currentTarget.style.borderColor = variant === 'primary' ? 'rgba(99,102,241,0.6)' : 'rgba(255,255,255,0.14)'
          e.currentTarget.style.background = variant === 'primary' ? 'rgba(99,102,241,0.35)' : 'rgba(255,255,255,0.04)'
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
      {children}
    </button>
  )
})
