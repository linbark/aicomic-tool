import { createApp } from 'vue'
import { createPinia } from 'pinia'
import router from './router'
import './style.css'
import App from './App.vue'

// #region agent log
fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'main.js:start',message:'main.js starting',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
// #endregion

try {
  const app = createApp(App)
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'main.js:app-created',message:'Vue app created',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
  // #endregion

  app.use(createPinia()) // 启用状态管理
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'main.js:pinia-added',message:'Pinia added',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
  // #endregion

  app.use(router)        // 启用路由
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'main.js:router-added',message:'Router added',data:{},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
  // #endregion

  const root = app.mount('#app')
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'main.js:mounted',message:'App mounted',data:{hasRoot:!!root},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
  // #endregion
} catch (error) {
  // #region agent log
  fetch('http://127.0.0.1:7242/ingest/fed868ac-0cc7-4d56-bc9f-30fe8b506df5',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'main.js:error',message:'App mount error',data:{error:error?.message,stack:error?.stack},timestamp:Date.now(),sessionId:'debug-session',runId:'run4',hypothesisId:'F'})}).catch(()=>{});
  // #endregion
  console.error('Failed to mount app:', error);
  document.body.innerHTML = '<div style="padding:20px;color:red;">应用加载失败: ' + (error?.message || '未知错误') + '</div>';
}