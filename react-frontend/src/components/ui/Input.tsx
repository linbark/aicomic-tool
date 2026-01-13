import { memo, forwardRef } from 'react'
import { inputStyle, transitions } from '../../styles/shared'

type InputProps = React.InputHTMLAttributes<HTMLInputElement>

export const Input = memo(forwardRef<HTMLInputElement, InputProps>(function Input({ style, ...props }, ref) {
  return (
    <input
      {...props}
      ref={ref}
      style={{
        ...inputStyle,
        transition: `border-color ${transitions.normal} ease`,
        ...style,
      }}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = 'rgba(99,102,241,0.6)'
        e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.3)'
        e.currentTarget.style.outlineOffset = '2px'
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = 'rgba(255,255,255,0.14)'
        e.currentTarget.style.outline = 'none'
      }}
    />
  )
}))
