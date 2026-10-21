import { memo, forwardRef } from 'react'
import { textareaStyle, transitions } from '../../styles/shared'

type TextareaProps = React.TextareaHTMLAttributes<HTMLTextAreaElement>

export const Textarea = memo(forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea({ style, ...props }, ref) {
  return (
    <textarea
      {...props}
      ref={ref}
      style={{
        ...textareaStyle,
        transition: `border-color ${transitions.normal} ease`,
        ...style,
      }}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = 'rgba(99,102,241,0.6)'
        e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.3)'
        e.currentTarget.style.outlineOffset = '2px'
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = 'rgba(255,255,255,0.12)'
        e.currentTarget.style.outline = 'none'
      }}
    />
  )
}))
