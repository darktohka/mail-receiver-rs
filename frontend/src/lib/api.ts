const API_BASE = '/api'

export function getApiKey(): string | null {
  return localStorage.getItem('apiKey')
}

export function setApiKey(key: string): void {
  localStorage.setItem('apiKey', key)
}

export function clearApiKey(): void {
  localStorage.removeItem('apiKey')
}

export class ApiError extends Error {
  status: number

  constructor(status: number, message: string) {
    super(message)
    this.status = status
    this.name = 'ApiError'
  }
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const apiKey = getApiKey()
  const queryChar = path.includes('?') ? '&' : '?'
  const url = `${API_BASE}${path}${apiKey ? `${queryChar}api_key=${encodeURIComponent(apiKey)}` : ''}`
  const res = await fetch(url, {
    ...init,
    headers: { ...init?.headers, 'Accept': 'application/json' },
  })
  if (res.status === 401) {
    clearApiKey()
    window.dispatchEvent(new CustomEvent('auth:logout'))
    throw new ApiError(401, 'Session expired')
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ message: res.statusText }))
    throw new ApiError(res.status, body.message ?? 'Unknown error')
  }
  return res.json()
}

export interface Recipient {
  domain: string
  name: string
  email: string
  messageCount: number
}

export interface MessageSummary {
  messageId: string
  processedAt: string
  from: string | null
  subject: string | null
  filename: string
  recipient: string
  message?: { href: string }
}

export interface AttachmentInfo {
  index: number
  filename: string | null
  contentType: string | null
  size: number
  contentId: string | null
  inline: boolean
}

export interface ParsedMail {
  attachments: AttachmentInfo[]
  headers: Record<string, unknown>
  headerLines: { key: string; line: string }[]
  html: string | null
  text: string | null
  textAsHtml: string | null
  subject: string | null
  date: string | null
  to: { text: string } | null
  from: { text: string } | null
  cc: { text: string } | null
  bcc: { text: string } | null
  replyTo: { text: string } | null
  messageId: string | null
  inReplyTo: string | null
  references: string | null
}

export async function fetchRecipients(): Promise<Recipient[]> {
  return apiFetch<Recipient[]>('/recipients')
}

export async function fetchWeeks(): Promise<string[]> {
  return apiFetch<string[]>('/weeks')
}

export async function fetchWeekMessages(year: number, week: number): Promise<MessageSummary[]> {
  return apiFetch<MessageSummary[]>(`/week/${year}/${week}`)
}

export async function fetchRecipientMessages(domain: string, name: string): Promise<MessageSummary[]> {
  return apiFetch<MessageSummary[]>(`/domain/${encodeURIComponent(domain)}/${encodeURIComponent(name)}`)
}

export async function fetchMessage(id: string): Promise<ParsedMail> {
  return apiFetch<ParsedMail>(`/message/${id}`)
}

export function getRawMessageUrl(id: string): string {
  const apiKey = getApiKey()
  return `${API_BASE}/message/${id}/raw?api_key=${encodeURIComponent(apiKey ?? '')}`
}

export function getAttachmentUrl(id: string, index: number): string {
  const apiKey = getApiKey()
  return `${API_BASE}/message/${id}/attachment/${index}?api_key=${encodeURIComponent(apiKey ?? '')}`
}

export function getAttachmentViewUrl(id: string, index: number): string {
  const apiKey = getApiKey()
  return `${API_BASE}/message/${id}/attachment/${index}?view=1&api_key=${encodeURIComponent(apiKey ?? '')}`
}
