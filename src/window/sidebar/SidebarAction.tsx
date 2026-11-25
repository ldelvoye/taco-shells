import type { JSX, ReactNode } from 'react'

interface SidebarActionProps {
  className: string
  label: string
  onActivate: () => void
  children: ReactNode
}

// Pressing leaves the keyboard in the terminal, and the click never reaches the row or header beneath.
export function SidebarAction({ className, label, onActivate, children }: SidebarActionProps): JSX.Element {
  return (
    <button
      type="button"
      className={`sidebar-action ${className}`}
      aria-label={label}
      title={label}
      onMouseDown={(event) => {
        event.preventDefault()
      }}
      onClick={(event) => {
        event.stopPropagation()
        onActivate()
      }}
    >
      {children}
    </button>
  )
}
