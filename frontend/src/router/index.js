import { createRouter, createWebHistory } from 'vue-router';
import ScriptView from '../views/ScriptView.vue';
import EventMatrixView from '../views/EventMatrixView.vue';
import EventFlowView from '../views/EventFlowView.vue';
import AssetLibraryView from '../views/AssetLibraryView.vue';

const routes = [
  { path: '/', redirect: '/script' },
  { path: '/script', component: ScriptView },
  { path: '/events', component: EventMatrixView },
  { path: '/events/flow', component: EventFlowView },
  { path: '/assets', component: AssetLibraryView },
];

export default createRouter({
  history: createWebHistory(),
  routes,
});