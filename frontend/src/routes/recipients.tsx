import { useQuery } from '@tanstack/react-query'
import { useParams, useNavigate } from '@tanstack/react-router'
import { fetchRecipients, fetchRecipientMessages, type Recipient, type MessageSummary } from '../lib/api'
import { ScrollArea } from '../components/ui/scroll-area'
import MessageDetail from '../components/MessageDetail'

function RecipientsThreePane({
  domain,
  name,
  messageId,
}: {
  domain?: string
  name?: string
  messageId?: string
}) {
  const navigate = useNavigate()

  const { data: recipients } = useQuery({
    queryKey: ['recipients'],
    queryFn: fetchRecipients,
  })

  const { data: messages } = useQuery({
    queryKey: ['recipientMessages', domain, name],
    queryFn: () => fetchRecipientMessages(domain!, name!),
    enabled: !!domain && !!name,
  })

  const selectRecipient = (d: string, n: string) => {
    navigate({ to: '/recipients/$domain/$name', params: { domain: d, name: n } })
  }

  const selectMessage = (id: string) => {
    if (!domain || !name) return
    navigate({
      to: '/recipients/$domain/$name/$messageId',
      params: { domain, name, messageId: id },
    })
  }

  const selectedRecipient = domain && name ? { domain, name } : null

  return (
    <div className="flex h-[calc(100dvh-3.5rem)] -mx-4 -mb-6">
      <div className="w-64 border-r shrink-0 flex flex-col">
        <div className="px-3 py-2 text-xs font-medium text-muted-foreground border-b">
          Recipients {recipients ? `(${recipients.length})` : ''}
        </div>
        <ScrollArea className="flex-1">
          {recipients?.map((r: Recipient) => (
            <button
              key={r.email}
              onClick={() => selectRecipient(r.domain, r.name)}
              className={`w-full text-left px-3 py-2 text-sm border-b hover:bg-accent/50 transition-colors ${
                r.domain === domain && r.name === name ? 'bg-accent font-medium' : ''
              }`}
            >
              <span className="truncate block">{r.email}</span>
              <span className="text-xs text-muted-foreground">{r.messageCount} messages</span>
            </button>
          ))}
        </ScrollArea>
      </div>

      <div className="w-80 border-r shrink-0 flex flex-col">
        <div className="px-3 py-2 text-xs font-medium text-muted-foreground border-b">
          {selectedRecipient
            ? `${selectedRecipient.name}@${selectedRecipient.domain} ${messages ? `(${messages.length})` : ''}`
            : 'Messages'}
        </div>
        <ScrollArea className="flex-1">
          {!selectedRecipient && (
            <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
              Select a recipient
            </div>
          )}
          {messages?.map((msg: MessageSummary) => (
            <button
              key={msg.messageId}
              onClick={() => selectMessage(msg.messageId)}
              className={`w-full text-left px-3 py-2.5 border-b hover:bg-accent/50 transition-colors ${
                msg.messageId === messageId ? 'bg-accent' : ''
              }`}
            >
              <p className="text-sm font-medium truncate">
                {msg.subject || <span className="italic text-muted-foreground">(no subject)</span>}
              </p>
              <p className="text-xs text-muted-foreground truncate mt-0.5">
                {msg.from || '(unknown sender)'}
              </p>
              <p className="text-xs text-muted-foreground mt-0.5">
                {new Date(msg.processedAt).toLocaleDateString(undefined, {
                  month: 'short',
                  day: 'numeric',
                  hour: '2-digit',
                  minute: '2-digit',
                })}
              </p>
            </button>
          ))}
        </ScrollArea>
      </div>

      <div className="flex-1 flex flex-col min-w-0">
        <MessageDetail messageId={messageId ?? null} />
      </div>
    </div>
  )
}

export default function RecipientsPage() {
  return <RecipientsThreePane />
}

export function RecipientMessagesPage() {
  const { domain, name } = useParams({ from: '/recipients/$domain/$name' })
  return <RecipientsThreePane domain={domain} name={name} />
}

export function RecipientMessageDetailPage() {
  const { domain, name, messageId } = useParams({
    from: '/recipients/$domain/$name/$messageId',
  })
  return <RecipientsThreePane domain={domain} name={name} messageId={messageId} />
}
