import { createRouter, createWebHistory } from 'vue-router';
import ScriptView from '../views/ScriptView.vue';
import EventMatrixView from '../views/EventMatrixView.vue';
import EventFlowView from '../views/EventFlowView.vue';
import CharacterView from '../views/CharacterView.vue';

const routes = [
  { path: '/', redirect: '/script' },
  { path: '/script', component: ScriptView },
  { path: '/events', component: EventMatrixView },
  { path: '/events/flow', component: EventFlowView },
  { path: '/characters', component: CharacterView },
];

export default createRouter({
  history: createWebHistory(),
  routes,
});