export {}

declare global {
  interface Window {
    __AICOMIC_API_BASE_URL__?: string
    setApiBaseUrl?: (url: string) => void
  }
}


