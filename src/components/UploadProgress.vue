<script setup>
import { computed } from 'vue'
import { useUploadStore } from '../stores/upload'

const store = useUploadStore()

const visible = computed(() => store.barVisible && store.tasks.length > 0)
const showRestore = computed(() => !store.barVisible && store.tasks.length > 0)
</script>

<template>
  <div>
    <!-- 收起后的恢复入口 -->
    <transition name="up-fade">
      <div v-if="showRestore" class="upload-restore" :title="'查看上传进度'" @click="store.showBar()">
        <span class="upload-restore-icon">&#xeb56;</span>
        <span class="upload-restore-text">{{ store.overallPercent }}%</span>
      </div>
    </transition>

    <transition name="up-slide">
      <div v-if="visible" class="upload-dock">
        <!-- 展开的任务列表 -->
        <div v-if="store.panelOpen" class="upload-panel">
          <div class="upload-panel-head">
            <span>{{ store.hasActive ? '上传任务' : '上传完成' }}</span>
            <button v-if="!store.hasActive" class="upload-link" @click="store.clear()">清空记录</button>
          </div>
          <div class="upload-task-list">
            <div v-for="task in store.tasks" :key="task.id" class="upload-task">
              <div class="upload-task-row">
                <span class="upload-task-title" :title="task.title">{{ task.title }}</span>
                <span class="upload-task-state" :class="task.status">
                  <template v-if="task.status === 'running'">{{ task.current }}/{{ task.total }}</template>
                  <template v-else-if="task.status === 'error'">失败</template>
                  <template v-else>成功 {{ task.success }}<template v-if="task.failed"> · 失败 {{ task.failed }}</template></template>
                </span>
              </div>
              <el-progress
                :percentage="task.status === 'done' ? 100 : task.progress"
                :status="task.status === 'error' ? 'exception' : (task.status === 'done' ? 'success' : undefined)"
                :stroke-width="6"
                :show-text="false"
              />
              <div v-if="task.fileName && task.status === 'running'" class="upload-task-file" :title="task.fileName">
                {{ task.fileName }}
              </div>
              <ul v-if="task.status !== 'running' && task.details.length" class="upload-task-details">
                <li v-for="(d, i) in task.details" :key="i" :class="d.success ? 'ok' : 'fail'">
                  <span class="d-name" :title="d.name">{{ d.name }}</span>
                  <span class="d-status">{{ d.success ? '成功' : (d.error || d.message || '失败') }}</span>
                </li>
              </ul>
            </div>
          </div>
        </div>

        <!-- 底部条 -->
        <div class="upload-bar">
          <div class="upload-bar-progress">
            <div class="upload-bar-fill" :style="{ width: store.overallPercent + '%' }" />
          </div>
          <div class="upload-bar-main" @click="store.togglePanel()">
            <span class="upload-bar-icon" :class="{ spinning: store.hasActive }">&#xeb56;</span>
            <span class="upload-bar-text">{{ store.summaryText }}</span>
            <span class="upload-bar-percent">{{ store.overallPercent }}%</span>
            <span class="upload-bar-spacer" />
            <button class="upload-bar-btn" :title="store.panelOpen ? '收起' : '查看详情'" @click.stop="store.togglePanel()">
              <span>{{ store.panelOpen ? '▾' : '▴' }}</span>
            </button>
            <button class="upload-bar-btn" title="隐藏到后台" @click.stop="store.hideBar()">
              <span>×</span>
            </button>
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.upload-dock {
  position: fixed;
  left: 196px;
  right: 0;
  bottom: 0;
  z-index: 2100;
  background: var(--bg-soft);
  border-top: 1px solid var(--border-soft);
  box-shadow: 0 -6px 20px rgba(0, 0, 0, 0.12);
}
.upload-bar {
  position: relative;
  height: 40px;
}
.upload-bar-progress {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: 3px;
  background: var(--bg-mute);
}
.upload-bar-fill {
  height: 100%;
  background: var(--primary);
  transition: width 0.25s ease;
}
.upload-bar-main {
  height: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  font-size: 13px;
  color: var(--text-2);
  cursor: pointer;
  user-select: none;
}
.upload-bar-icon {
  font-family: 'iconfont';
  color: var(--primary);
  font-size: 15px;
}
.upload-bar-icon.spinning {
  animation: up-pulse 1.2s infinite;
}
@keyframes up-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.45; }
}
.upload-bar-text {
  font-weight: 600;
  color: var(--text-1);
}
.upload-bar-percent {
  color: var(--primary);
  font-weight: 600;
}
.upload-bar-spacer {
  flex: 1;
}
.upload-bar-btn {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-soft);
  border-radius: 4px;
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  transition: all 0.15s;
  opacity: 0;
  pointer-events: none;
}
.upload-bar-main:hover .upload-bar-btn,
.upload-bar-main:focus-within .upload-bar-btn {
  opacity: 1;
  pointer-events: auto;
}
.upload-bar-btn:hover {
  color: var(--primary);
  border-color: var(--primary);
}
.upload-panel {
  max-height: 260px;
  overflow-y: auto;
  padding: 10px 16px 12px;
  border-bottom: 1px solid var(--border-soft);
}
.upload-panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  margin-bottom: 8px;
}
.upload-link {
  border: none;
  background: transparent;
  color: var(--primary);
  font-size: 12px;
  cursor: pointer;
}
.upload-task {
  padding: 8px 0;
  border-top: 1px dashed var(--border-soft);
}
.upload-task:first-child {
  border-top: none;
}
.upload-task-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 4px;
}
.upload-task-title {
  font-size: 12px;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.upload-task-state {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-3);
}
.upload-task-state.done { color: var(--el-color-success); }
.upload-task-state.error { color: var(--el-color-danger); }
.upload-task-file {
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.upload-task-details {
  list-style: none;
  margin: 6px 0 0;
  padding: 0;
  max-height: 110px;
  overflow-y: auto;
}
.upload-task-details li {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 11px;
  padding: 2px 0;
}
.upload-task-details li.ok .d-status { color: var(--el-color-success); }
.upload-task-details li.fail .d-status { color: var(--el-color-danger); }
.d-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-2);
}
.upload-restore {
  position: fixed;
  right: 18px;
  bottom: 18px;
  z-index: 2100;
  height: 34px;
  padding: 0 12px;
  display: flex;
  align-items: center;
  gap: 6px;
  border-radius: 17px;
  background: var(--bg-soft);
  border: 1px solid var(--primary);
  color: var(--primary);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.18);
}
.upload-restore-icon {
  font-family: 'iconfont';
  font-size: 14px;
}
.up-slide-enter-active,
.up-slide-leave-active,
.up-fade-enter-active,
.up-fade-leave-active {
  transition: all 0.2s ease;
}
.up-slide-enter-from,
.up-slide-leave-to {
  transform: translateY(100%);
  opacity: 0;
}
.up-fade-enter-from,
.up-fade-leave-to {
  opacity: 0;
}
</style>
