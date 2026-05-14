import { createRootRoute, createRoute } from '@tanstack/react-router'
import RootLayout from './routes/__root'
import LoginPage from './routes/login'
import RecipientsPage, {
  RecipientMessagesPage,
  RecipientMessageDetailPage,
} from './routes/recipients'
import WeeklyPage, {
  WeekMessagesPage,
  WeekMessageDetailPage,
} from './routes/weekly'

function RedirectToRecipients() {
  window.location.href = '/recipients'
  return null
}

const rootRoute = createRootRoute({
  component: RootLayout,
})

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: RedirectToRecipients,
})

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/login',
  component: LoginPage,
})

const recipientsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/recipients',
  component: RecipientsPage,
})

const recipientMessagesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/recipients/$domain/$name',
  component: RecipientMessagesPage,
})

const recipientMessageDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/recipients/$domain/$name/$messageId',
  component: RecipientMessageDetailPage,
})

const weeklyRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/weekly',
  component: WeeklyPage,
})

const weekMessagesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/weekly/$year/$week',
  component: WeekMessagesPage,
})

const weekMessageDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/weekly/$year/$week/$messageId',
  component: WeekMessageDetailPage,
})

const routeTree = rootRoute.addChildren([
  indexRoute,
  loginRoute,
  recipientsRoute,
  recipientMessagesRoute,
  recipientMessageDetailRoute,
  weeklyRoute,
  weekMessagesRoute,
  weekMessageDetailRoute,
])

export { routeTree }
