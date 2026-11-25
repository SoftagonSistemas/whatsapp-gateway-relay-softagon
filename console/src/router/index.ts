import { createRouter, createWebHistory } from 'vue-router';
import { useAuth } from '../composables/useAuth';
import LoginView from '../views/LoginView.vue';
import HomeView from '../views/HomeView.vue';
import ConnectView from '../views/ConnectView.vue';
import SessionView from '../views/SessionView.vue';
import EventsView from '../views/EventsView.vue';

export const router = createRouter({
  history: createWebHistory('/console/'),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: LoginView,
      meta: { public: true },
    },
    {
      path: '/',
      name: 'home',
      component: HomeView,
    },
    {
      path: '/connect',
      name: 'connect',
      component: ConnectView,
    },
    {
      path: '/sessions/:id',
      name: 'session',
      component: SessionView,
    },
    {
      path: '/events',
      name: 'events',
      component: EventsView,
    },
    {
      path: '/api',
      name: 'api',
      // Lazy-loaded so js-yaml stays out of the initial JS bundle
      component: () => import('../views/ApiReferenceView.vue'),
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
});

router.beforeEach((to) => {
  const { isAuthenticated } = useAuth();
  if (!to.meta.public && !isAuthenticated.value) {
    return {
      name: 'login',
      query: to.fullPath && to.fullPath !== '/' ? { redirect: to.fullPath } : undefined,
    };
  }
  if (to.name === 'login' && isAuthenticated.value) {
    return { name: 'home' };
  }
  return true;
});
