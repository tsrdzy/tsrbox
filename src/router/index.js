import { createRouter, createWebHashHistory } from 'vue-router'
import { useStore } from '../composables/useStore'
import ModelLayout from '../views/ModelLayout.vue'
import LibrarySelectView from '../views/LibrarySelectView.vue'

const routes = [
  {
    path: '/',
    name: 'home',
    meta: { requiresAuth: false }
  },
  {
    path: '/select-library',
    name: 'select-library',
    component: LibrarySelectView,
    meta: { requiresAuth: false }
  },
  {
    path: '/model',
    name: 'model',
    component: ModelLayout,
    meta: { icon: '&#xe82a;', title: 'sidebar.model', requiresAuth: true }
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('../views/SettingsView.vue'),
    meta: { icon: '&#xe643;', title: 'menu.settings', requiresAuth: true }
  },
  {
    path: '/shortcuts',
    name: 'shortcuts',
    component: () => import('../views/ShortcutsView.vue'),
    meta: { icon: '&#xe60d;', title: 'menu.shortcuts', requiresAuth: true }
  },
  {
    path: '/logs',
    name: 'logs',
    component: () => import('../views/LogsView.vue'),
    meta: { icon: '&#xe62c;', title: 'menu.operationLogs', requiresAuth: true }
  },
  {
    path: '/recycle',
    name: 'recycle',
    component: () => import('../views/RecycleView.vue'),
    meta: { icon: '&#xeb6c;', title: 'recycle.title', requiresAuth: true }
  }
  // 插件路由由插件通过 router.addRoute 自行注册（见 pluginManager）
]

const router = createRouter({
  history: createWebHashHistory(),
  routes
})

router.beforeEach(async (to) => {
  const { state, checkStatus } = useStore()

  if (!state.libraries.length || state.view === 'init') {
    try {
      await checkStatus()
    } catch (e) {
      console.error('checkStatus failed:', e)
    }
  }

  if (!state.initialized && to.meta.requiresAuth) {
    return '/select-library'
  }

  if (to.path === '/') {
    return state.initialized ? '/model' : '/select-library'
  }
})

export default router
