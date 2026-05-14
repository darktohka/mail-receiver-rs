import { Outlet, Link, useLocation } from '@tanstack/react-router'
import { getApiKey, clearApiKey } from '../lib/api'
import { Button } from '../components/ui/button'
import { useState, useEffect, useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Toaster, toast } from 'sonner'
import { RefreshCw, LogOut } from 'lucide-react'

export default function RootLayout() {
  const [apiKey, setApiKey] = useState<string | null>(getApiKey)
  const [refreshing, setRefreshing] = useState(false)
  const location = useLocation()
  const queryClient = useQueryClient()

  useEffect(() => {
    const check = () => setApiKey(getApiKey())
    window.addEventListener('storage', check)
    return () => window.removeEventListener('storage', check)
  }, [])

  useEffect(() => {
    const onLogout = () => {
      clearApiKey()
      setApiKey(null)
      toast.error('Session expired. Please log in again.')
    }
    window.addEventListener('auth:logout', onLogout)
    return () => window.removeEventListener('auth:logout', onLogout)
  }, [])

  const isLoginPage = location.pathname === '/login'

  if (!apiKey && !isLoginPage) {
    window.location.href = '/login'
    return null
  }

  const handleLogout = () => {
    clearApiKey()
    setApiKey(null)
    window.location.href = '/login'
  }

  const handleRefresh = useCallback(async () => {
    setRefreshing(true)
    await queryClient.invalidateQueries()
    setTimeout(() => setRefreshing(false), 400)
  }, [queryClient])

  const tabs = [
    { path: '/recipients', label: 'E-mail Addresses' },
    { path: '/weekly', label: 'Weekly View' },
  ]

  return (
    <div className="min-h-dvh bg-background">
      <Toaster richColors position="top-right" />
      {!isLoginPage && (
        <header className="border-b sticky top-0 bg-background z-10">
          <div className="max-w-7xl mx-auto px-4 h-14 flex items-center gap-6">
            <a href="https://github.com/darktohka/mail-receiver-rs" target="_blank" rel="noopener noreferrer" className="font-bold text-lg tracking-tight hover:underline">
              Mailbox
            </a>
            <nav className="flex items-center gap-1 flex-1">
              {tabs.map((tab) => (
                <Link
                  key={tab.path}
                  to={tab.path as '/recipients' | '/weekly'}
                  className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors ${
                    location.pathname.startsWith(tab.path)
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                  }`}
                >
                  {tab.label}
                </Link>
              ))}
            </nav>
            <Button variant="ghost" size="sm" onClick={handleRefresh} disabled={refreshing}>
              <RefreshCw className={`h-4 w-4 ${refreshing ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
            <Button variant="outline" size="sm" onClick={handleLogout}>
              <LogOut className="h-4 w-4 mr-1.5" />
              Logout
            </Button>
          </div>
        </header>
      )}
      <main className={isLoginPage ? '' : 'max-w-7xl mx-auto px-4 py-6'}>
        <Outlet />
      </main>
    </div>
  )
}
