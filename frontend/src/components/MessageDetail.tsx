import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { fetchMessage, getRawMessageUrl, getAttachmentUrl, clearApiKey } from '../lib/api'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs'
import { Separator } from './ui/separator'
import { ScrollArea } from './ui/scroll-area'
import { Paperclip, Download } from 'lucide-react'

function formatDate(dateStr: string | null): string {
  if (!dateStr) return ''
  const d = new Date(dateStr)
  return d.toLocaleDateString(undefined, {
    weekday: 'long',
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

interface Props {
  messageId: string | null
}

export default function MessageDetail({ messageId }: Props) {
  const [raw, setRaw] = useState<string | null>(null)
  const [rawLoading, setRawLoading] = useState(false)

  const { data: msg, isLoading, error } = useQuery({
    queryKey: ['message', messageId],
    queryFn: () => fetchMessage(messageId!),
    enabled: !!messageId,
  })

  const loadRaw = async () => {
    if (!messageId || raw || rawLoading) return
    setRawLoading(true)
    try {
      const res = await fetch(getRawMessageUrl(messageId))
      if (res.status === 401) {
        clearApiKey()
        window.dispatchEvent(new CustomEvent('auth:logout'))
        setRaw('Session expired')
        return
      }
      setRaw(await res.text())
    } catch {
      setRaw('Failed to load raw message')
    } finally {
      setRawLoading(false)
    }
  }

  if (!messageId) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground">
        <p>Select a message to view</p>
      </div>
    )
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground">
        <p>Loading message...</p>
      </div>
    )
  }

  if (error || !msg) {
    return (
      <div className="flex items-center justify-center h-full text-destructive">
        <p>Message not found</p>
      </div>
    )
  }

  const hasAttachments = msg.attachments && msg.attachments.length > 0

  return (
    <Tabs defaultValue="gmail" onValueChange={(v: string) => { if (v === 'raw') loadRaw() }}>
      <div className="border-b px-4 py-2 sticky top-0 bg-background z-10">
        <TabsList>
          <TabsTrigger value="gmail">Message</TabsTrigger>
          <TabsTrigger value="raw">Raw</TabsTrigger>
        </TabsList>
      </div>

      <TabsContent value="gmail" className="m-0 p-0">
        <ScrollArea className="h-full">
          <div className="p-4 space-y-4">
            <h1 className="text-xl font-semibold leading-tight">
              {msg.subject || <span className="italic text-muted-foreground">(no subject)</span>}
            </h1>

            <Separator />

            <div className="space-y-1.5 text-sm">
              <div className="flex gap-2">
                <span className="text-muted-foreground w-14 shrink-0">From:</span>
                <span className="font-medium">{msg.from?.text || '(unknown)'}</span>
              </div>
              <div className="flex gap-2">
                <span className="text-muted-foreground w-14 shrink-0">To:</span>
                <span>{msg.to?.text || '(unknown)'}</span>
              </div>
              {msg.cc && (
                <div className="flex gap-2">
                  <span className="text-muted-foreground w-14 shrink-0">CC:</span>
                  <span>{msg.cc.text}</span>
                </div>
              )}
              <div className="flex gap-2">
                <span className="text-muted-foreground w-14 shrink-0">Date:</span>
                <span>{formatDate(msg.date)}</span>
              </div>
            </div>

            {hasAttachments && (
              <>
                <Separator />
                <div className="space-y-2">
                  <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
                    <Paperclip className="h-4 w-4" />
                    <span>{msg.attachments.length} attachment{msg.attachments.length > 1 ? 's' : ''}</span>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {msg.attachments.map((att) => (
                      <a
                        key={att.index}
                        href={getAttachmentUrl(messageId, att.index)}
                        download={att.filename ?? `attachment-${att.index}`}
                        className="flex items-center gap-2 rounded-lg border p-2.5 hover:bg-accent transition-colors text-sm min-w-0"
                      >
                        <Download className="h-4 w-4 shrink-0 text-muted-foreground" />
                        <span className="truncate font-medium">{att.filename || `attachment-${att.index}`}</span>
                        <span className="text-xs text-muted-foreground shrink-0">{formatSize(att.size)}</span>
                      </a>
                    ))}
                  </div>
                </div>
              </>
            )}

            <Separator />

            {msg.html ? (
              <div
                className="prose prose-sm max-w-none dark:prose-invert"
                dangerouslySetInnerHTML={{ __html: msg.html }}
              />
            ) : msg.textAsHtml ? (
              <div
                className="prose prose-sm max-w-none dark:prose-invert"
                dangerouslySetInnerHTML={{ __html: msg.textAsHtml }}
              />
            ) : msg.text ? (
              <pre className="whitespace-pre-wrap text-sm font-sans">{msg.text}</pre>
            ) : (
              <p className="text-muted-foreground italic">(empty message)</p>
            )}
          </div>
        </ScrollArea>
      </TabsContent>

      <TabsContent value="raw" className="m-0 p-0">
        <ScrollArea className="h-full">
          <div className="p-4">
            {rawLoading ? (
              <div className="text-center text-muted-foreground py-8">Loading raw message...</div>
            ) : raw ? (
              <pre className="text-xs font-mono whitespace-pre-wrap break-all">{raw}</pre>
            ) : (
              <div className="text-center text-muted-foreground py-8">
                Click Raw tab to load the raw message
              </div>
            )}
          </div>
        </ScrollArea>
      </TabsContent>
    </Tabs>
  )
}
