<template>
  <div class="up-wrap">
    <!-- 收起后：标题栏右侧的小胶囊，点击展开 -->
    <button v-if="store.restoreVisible" class="upload-restore" @click="store.restorePanel">
      <span class="restore-spinner" :class="{ done: store.overallPercent >= 100 }">
        <svg v-if="store.overallPercent < 100" viewBox="0 0 20 20" fill="none">
          <circle cx="10" cy="10" r="8" stroke="currentColor" stroke-width="2" opacity="0.3" />
          <path d="M18 10a8 8 0 0 0-8-8" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
        </svg>
        <svg v-else viewBox="0 0 20 20" fill="none">
          <circle cx="10" cy="10" r="8" stroke="currentColor" stroke-width="2" />
          <path d="M6.5 10.5l2.5 2.5 4.5-5.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </span>
      <span class="restore-text">{{ store.restoreLabel }}</span>
    </button>

    <!-- 展开的任务面板：从标题栏向下弹出 -->
    <div v-if="store.dockVisible" class="upload-dock">
      <div class="upload-bar">
        <div class="upload-summary" @click="store.togglePanel">
          <span class="upload-spinner" v-if="store.overallPercent < 100">
            <svg viewBox="0 0 20 20" fill="none">
              <circle cx="10" cy="10" r="8" stroke="currentColor" stroke-width="2" opacity="0.3" />
              <path d="M18 10a8 8 0 0 0-8-8" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
            </svg>
          </span>
          <span class="upload-title">{{ store.barTitle }}</span>
          <span class="upload-overall">{{ store.overallPercent }}%</span>
        </div>
        <div class="upload-actions">
          <button class="upload-btn" @click="store.togglePanel">
            <svg viewBox="0 0 20 20" fill="none" :class="{ collapsed: !store.panelOpen }">
              <path d="M5 8l5 5 5-5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          </button>
          <button class="upload-btn" @click="store.minimizeToRestore">
            <svg viewBox="0 0 20 20" fill="none">
              <path d="M4 10h12" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
            </svg>
          </button>
        </div>
      </div>
      <!-- 汇总进度条 -->
      <div class="bar-track">
        <div
          class="bar-fill"
          :class="{ done: store.overallPercent >= 100 }"
          :style="{ width: store.overallPercent + '%' }"
        ></div>
      </div>
      <div v-if="store.panelOpen" class="upload-panel">
        <div
          v-for="task in store.tasks"
          :key="task.id"
          class="upload-task"
          :class="{ done: task.status === 'done', error: task.status === 'error' }"
        >
          <div class="task-info">
            <span class="task-name" :title="task.fileName">{{ task.fileName }}</span>
            <span class="task-meta">{{ task.detail }}</span>
          </div>
          <div class="task-right">
            <span v-if="task.status === 'done'" class="task-check">
              <svg viewBox="0 0 20 20" fill="none">
                <path d="M4 10.5l4 4 8-9" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
            </span>
            <span v-else-if="task.status === 'error'" class="task-error-text">{{ t('common.failed') }}</span>
            <span v-else class="task-percent">{{ task.progress }}%</span>
          </div>
          <div class="task-track" v-if="task.status === 'running'">
            <div class="task-fill" :style="{ width: task.progress + '%' }"></div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useI18n } from 'vue-i18n'
import { useUploadStore } from '../stores/upload'

const { t } = useI18n()
const store = useUploadStore()
</script>

<style scoped>
/* 标题栏内联容器 */
.up-wrap {
  position: relative;
  display: inline-flex;
  align-items: center;
  margin-right: 6px;
  z-index: 2200;
}

/* 标题栏小胶囊 */
.upload-restore {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 10px;
  border-radius: 12px;
  border: 1px solid var(--border-soft);
  background: var(--panel);
  color: var(--text-primary);
  cursor: pointer;
  font-size: 11px;
  line-height: 1;
  font-family: inherit;
  user-select: none;
  transition: background 0.2s, box-shadow 0.2s;
}
.upload-restore:hover {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.18);
}
.restore-spinner {
  width: 13px;
  height: 13px;
  display: inline-flex;
  align-items: center;
  color: var(--brand);
}
.restore-spinner svg {
  width: 13px;
  height: 13px;
  animation: up-spin 1s linear infinite;
}
.restore-spinner.done {
  color: var(--el-color-success, #67c23a);
}
.restore-spinner.done svg {
  animation: none;
}
.restore-text {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 下拉面板的壳：绝对定位挂到标题栏下沿 */
.upload-dock {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  width: 340px;
  max-width: 70vw;
  background: var(--panel);
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
  overflow: hidden;
}
.upload-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
}
.upload-summary {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  cursor: pointer;
}
.upload-spinner {
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  color: var(--brand);
}
.upload-spinner svg {
  width: 16px;
  height: 16px;
  animation: up-spin 1s linear infinite;
}
@keyframes up-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
.upload-title {
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.upload-overall {
  font-size: 12px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
.upload-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}
.upload-btn {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 5px;
}
.upload-btn:hover {
  background: var(--bg-soft);
  color: var(--text-primary);
}
.upload-btn svg {
  width: 14px;
  height: 14px;
  transition: transform 0.2s;
}
.upload-btn svg.collapsed {
  transform: rotate(-90deg);
}

/* 汇总进度条 */
.bar-track {
  height: 2px;
  background: var(--border-soft);
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  background: var(--brand);
  transition: width 0.3s;
}
.bar-fill.done {
  background: var(--el-color-success, #67c23a);
}

/* 任务面板在进度条下方（向下展开） */
.upload-panel {
  max-height: 280px;
  overflow-y: auto;
  padding: 8px 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-top: 1px solid var(--border-soft);
}
.upload-task {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px 10px;
  border-radius: 6px;
  background: var(--bg-soft);
}
.upload-task.error {
  background: rgba(245, 108, 108, 0.12);
}
.task-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.task-name {
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.task-meta {
  font-size: 11px;
  color: var(--text-secondary);
}
.task-right {
  margin-left: 10px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
}
.task-percent {
  font-size: 11px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
.task-check {
  width: 15px;
  height: 15px;
  display: inline-flex;
  align-items: center;
  color: var(--el-color-success, #67c23a);
}
.task-check svg {
  width: 15px;
  height: 15px;
}
.task-error-text {
  font-size: 11px;
  color: var(--el-color-error, #f56c6c);
}
.task-track {
  position: absolute;
  left: 8px;
  right: 8px;
  bottom: 4px;
  height: 2px;
  border-radius: 1px;
  background: var(--border-soft);
  overflow: hidden;
}
.task-fill {
  height: 100%;
  background: var(--brand);
  transition: width 0.3s;
}
</style>
