import { createRouter, createWebHistory } from 'vue-router';
import ScriptView from '../views/ScriptView.vue';
import EventFlowView from '../views/EventFlowView.vue';
import CharacterView from '../views/CharacterView.vue';

const routes = [
  { path: '/', redirect: '/script' },
  { path: '/script', component: ScriptView },
  { path: '/events', component: EventFlowView },
  { path: '/characters', component: CharacterView },
];

export default createRouter({
  history: createWebHistory(),
  routes,
});