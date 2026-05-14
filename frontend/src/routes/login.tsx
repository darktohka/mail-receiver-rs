import { useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { setApiKey, getApiKey, ApiError } from '../lib/api'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card'

export default function LoginPage() {
  const [key, setKey] = useState(getApiKey() ?? '')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)

    try {
      const res = await fetch(`/api/recipients?api_key=${encodeURIComponent(key)}`)
      if (!res.ok) {
        const body = await res.json().catch(() => ({ message: 'Invalid API key' }))
        throw new ApiError(res.status, body.message ?? 'Invalid API key')
      }
      setApiKey(key)
      window.dispatchEvent(new Event('storage'))
      navigate({ to: '/recipients' })
    } catch (err) {
      localStorage.removeItem('apiKey')
      setError(err instanceof ApiError ? err.message : 'Connection failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-dvh flex items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="text-center">
          <CardTitle className="text-2xl">Mail Receiver</CardTitle>
          <CardDescription>Enter your API key to continue</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <Input
              type="password"
              placeholder="API Key"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              autoFocus
            />
            {error && <p className="text-sm text-destructive">{error}</p>}
            <Button type="submit" className="w-full" disabled={loading || !key}>
              {loading ? 'Verifying...' : 'Login'}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
