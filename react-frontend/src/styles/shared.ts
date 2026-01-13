/**
 * 共享样式常量
 * 统一管理 UI 样式，便于维护和一致性
 */

export const colors = {
  // 背景色
  bg: {
    primary: '#0b1220',
    secondary: 'rgba(255,255,255,0.04)',
    tertiary: 'rgba(0,0,0,0.12)',
    hover: 'rgba(255,255,255,0.08)',
  },
  // 边框色
  border: {
    default: 'rgba(255,255,255,0.08)',
    light: 'rgba(255,255,255,0.14)',
    active: 'rgba(99,102,241,0.6)',
  },
  // 文字色
  text: {
    primary: 'rgba(255,255,255,0.92)',
    secondary: 'rgba(255,255,255,0.7)',
    muted: 'rgba(255,255,255,0.5)',
    error: '#f87171',
    success: '#34d399',
  },
  // 主色调
  primary: {
    bg: 'rgba(99,102,241,0.35)',
    border: 'rgba(99,102,241,0.6)',
    hover: 'rgba(99,102,241,0.45)',
  },
} as const

export const spacing = {
  xs: 4,
  sm: 6,
  md: 8,
  lg: 10,
  xl: 12,
  '2xl': 16,
  '3xl': 20,
} as const

export const borderRadius = {
  sm: 8,
  md: 10,
  lg: 12,
} as const

export const transitions = {
  fast: '150ms',
  normal: '200ms',
  slow: '300ms',
} as const

/**
 * 基础按钮样式
 */
export const baseButtonStyle: React.CSSProperties = {
  borderRadius: borderRadius.md,
  padding: `${spacing.md}px ${spacing.lg}px`,
  cursor: 'pointer',
  border: `1px solid ${colors.border.light}`,
  background: colors.bg.secondary,
  color: colors.text.primary,
  fontSize: 12,
  fontWeight: 500,
  transition: `all ${transitions.normal} ease`,
  outline: 'none',
}

/**
 * 主按钮样式
 */
export const primaryButtonStyle: React.CSSProperties = {
  ...baseButtonStyle,
  background: colors.primary.bg,
  border: `1px solid ${colors.primary.border}`,
  color: '#fff',
}

/**
 * 输入框样式
 */
export const inputStyle: React.CSSProperties = {
  width: '100%',
  borderRadius: borderRadius.md,
  border: `1px solid ${colors.border.default}`,
  background: 'rgba(0,0,0,0.25)',
  color: colors.text.primary,
  padding: `${spacing.md}px ${spacing.lg}px`,
  outline: 'none',
  fontSize: 12,
  transition: `border-color ${transitions.normal} ease`,
  boxSizing: 'border-box' as const,
}

/**
 * 文本域样式
 */
export const textareaStyle: React.CSSProperties = {
  ...inputStyle,
  background: 'rgba(0,0,0,0.22)',
  border: `1px solid rgba(255,255,255,0.12)`,
  resize: 'vertical' as const,
  lineHeight: 1.5,
  fontFamily: 'inherit',
}

/**
 * 面板样式
 */
export const panelStyle: React.CSSProperties = {
  border: `1px solid ${colors.border.default}`,
  borderRadius: borderRadius.lg,
  background: colors.bg.secondary,
  padding: spacing.xl,
}

/**
 * 卡片样式
 */
export const cardStyle: React.CSSProperties = {
  border: `1px solid ${colors.border.default}`,
  borderRadius: borderRadius.md,
  background: colors.bg.tertiary,
  padding: spacing.lg,
}

/**
 * Hover 效果工具函数
 * 注意：React 内联样式不支持伪类选择器
 * 如需 hover 效果，请在组件中使用 onMouseEnter/onMouseLeave
 */

/**
 * Focus 样式（用于可访问性）
 */
export const focusStyle: React.CSSProperties = {
  outline: `2px solid ${colors.primary.border}`,
  outlineOffset: '2px',
}
