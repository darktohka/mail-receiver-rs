import { useQuery } from '@tanstack/react-query'
import { useParams, useNavigate } from '@tanstack/react-router'
import { fetchWeeks, fetchWeekMessages, type MessageSummary } from '../lib/api'
import { ScrollArea } from '../components/ui/scroll-area'
import MessageDetail from '../components/MessageDetail'
import { useMemo } from 'react'

function parseWeek(name: string): { year: number; week: number } | null {
  const m = name.match(/^w(\d+)-(\d+)$/)
  if (!m) return null
  return { week: parseInt(m[1]), year: parseInt(m[2]) }
}

function weekStartDate(year: number, week: number): Date {
  const jan1 = new Date(year, 0, 1)
  const days = (week - 1) * 7
  const firstDay = jan1.getDay()
  const mondayOffset = firstDay === 0 ? -6 : 1 - firstDay
  return new Date(year, 0, 1 + mondayOffset + days)
}

function formatDateRange(year: number, week: number): string {
  const start = weekStartDate(year, week)
  const end = new Date(start)
  end.setDate(end.getDate() + 6)
  const opts: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric' }
  return `${start.toLocaleDateString(undefined, opts)} – ${end.toLocaleDateString(undefined, opts)}`
}

function WeeklyThreePane({
  year,
  week,
  messageId,
}: {
  year?: string
  week?: string
  messageId?: string
}) {
  const navigate = useNavigate()
  const yearNum = year ? parseInt(year) : undefined
  const weekNum = week ? parseInt(week) : undefined

  const { data: weeks } = useQuery({
    queryKey: ['weeks'],
    queryFn: fetchWeeks,
  })

  const { data: messages } = useQuery({
    queryKey: ['weekMessages', yearNum, weekNum],
    queryFn: () => fetchWeekMessages(yearNum!, weekNum!),
    enabled: !!yearNum && !!weekNum,
  })

  const parsed = useMemo(() => {
    if (!weeks) return []
    return weeks
      .map((name) => ({ name, parsed: parseWeek(name) }))
      .filter(
        (w): w is { name: string; parsed: NonNullable<ReturnType<typeof parseWeek>> } =>
          w.parsed !== null,
      )
      .sort((a, b) => {
        if (a.parsed.year !== b.parsed.year) return b.parsed.year - a.parsed.year
        return b.parsed.week - a.parsed.week
      })
  }, [weeks])

  const selectWeek = (y: number, w: number) => {
    navigate({ to: '/weekly/$year/$week', params: { year: String(y), week: String(w) } })
  }

  const selectMessage = (id: string) => {
    if (!yearNum || !weekNum) return
    navigate({
      to: '/weekly/$year/$week/$messageId',
      params: { year: String(yearNum), week: String(weekNum), messageId: id },
    })
  }

  const selectedWeek = yearNum && weekNum ? { year: yearNum, week: weekNum } : null

  return (
    <div className="flex h-[calc(100dvh-3.5rem)] -mx-4 -mb-6">
      <div className="w-64 border-r shrink-0 flex flex-col">
        <div className="px-3 py-2 text-xs font-medium text-muted-foreground border-b">
          Weeks {parsed ? `(${parsed.length})` : ''}
        </div>
        <ScrollArea className="flex-1">
          {parsed.map((w) => (
            <button
              key={w.name}
              onClick={() => selectWeek(w.parsed.year, w.parsed.week)}
              className={`w-full text-left px-3 py-2 text-sm border-b hover:bg-accent/50 transition-colors ${
                w.parsed.year === yearNum && w.parsed.week === weekNum
                  ? 'bg-accent font-medium'
                  : ''
              }`}
            >
              <span className="truncate block">
                Week {w.parsed.week}, {w.parsed.year}
              </span>
              <span className="text-xs text-muted-foreground">
                {formatDateRange(w.parsed.year, w.parsed.week)}
              </span>
            </button>
          ))}
          {!parsed.length && (
            <div className="flex items-center justify-center h-full text-sm text-muted-foreground px-3">
              No weeks available
            </div>
          )}
        </ScrollArea>
      </div>

      <div className="w-80 border-r shrink-0 flex flex-col">
        <div className="px-3 py-2 text-xs font-medium text-muted-foreground border-b">
          {selectedWeek
            ? `Week ${selectedWeek.week}, ${selectedWeek.year} ${messages ? `(${messages.length})` : ''}`
            : 'Messages'}
        </div>
        <ScrollArea className="flex-1">
          {!selectedWeek && (
            <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
              Select a week
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
              <p className="text-xs text-muted-foreground mt-0.5">{msg.recipient}</p>
              <p className="text-xs text-muted-foreground">
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

export default function WeeklyPage() {
  return <WeeklyThreePane />
}

export function WeekMessagesPage() {
  const { year, week } = useParams({ from: '/weekly/$year/$week' })
  return <WeeklyThreePane year={year} week={week} />
}

export function WeekMessageDetailPage() {
  const { year, week, messageId } = useParams({ from: '/weekly/$year/$week/$messageId' })
  return <WeeklyThreePane year={year} week={week} messageId={messageId} />
}
