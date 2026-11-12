<script setup>
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox, ElLoading } from 'element-plus'
import { useStore } from '../composables/useStore'
import { useCovers } from '../composables/useCovers'

const { t } = useI18n()
const router = useRouter()
const { loadAll } = useStore()
const covers = useCovers()

const models = ref([])
const retentionDays = ref(30)
const loading = ref(false)
const emptying = ref(false)

async function load() {
  loading.value = true
  try {
    const res = await window.api.models.listDeleted()
    models.value = res?.models || []
    retentionDays.value = Number(res?.retentionDays) || 30
    // 封面懒加载进共享缓存，列表接口不再内嵌 base64
    models.value.forEach((m) => covers.ensureCover(m))
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  } finally {
    loading.value = false
  }
}

function formatDate(iso) {
  if (!iso) return ''
  const date = new Date(iso)
  if (!Number.isFinite(date.getTime())) return ''
  const pad = (value) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

function formatSize(bytes) {
  const size = Number(bytes || 0)
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / 1024 / 1024).toFixed(2)} MB`
}

function daysLeft(model) {
  const time = new Date(model.deletedAt).getTime()
  if (!Number.isFinite(time)) return retentionDays.value
  const elapsed = (Date.now() - time) / 86400000
  return Math.max(0, Math.ceil(retentionDays.value - elapsed))
}

async function restore(model) {
  try {
    await window.api.models.restore(model.id)
    ElMessage.success(t('recycle.restored'))
    await load()
    await loadAll()
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

async function remove(model) {
  try {
    await ElMessageBox.confirm(
      t('recycle.deleteConfirm', { name: model.name || model.fileName }),
      t('recycle.deleteForever'),
      {
        confirmButtonText: t('recycle.deleteForever'),
        cancelButtonText: t('common.cancel'),
        type: 'warning',
        confirmButtonClass: 'el-button--danger'
      }
    )
  } catch {
    return
  }
  try {
    await window.api.models.deleteForever(model.id)
    ElMessage.success(t('recycle.deleted'))
    await load()
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  }
}

async function emptyAll() {
  if (!models.value.length || emptying.value) return
  try {
    await ElMessageBox.confirm(t('recycle.emptyConfirm'), t('recycle.emptyBin'), {
      confirmButtonText: t('recycle.emptyBin'),
      cancelButtonText: t('common.cancel'),
      type: 'warning',
      confirmButtonClass: 'el-button--danger'
    })
  } catch {
    return
  }
  emptying.value = true
  const mask = ElLoading.service({
    lock: true,
    text: t('recycle.emptying', { count: models.value.length }),
    background: 'rgba(0, 0, 0, 0.35)'
  })
  try {
    const res = await window.api.models.emptyRecycle()
    ElMessage.success(t('recycle.emptied', { count: res?.removed || 0 }))
    models.value = []
    await load()
  } catch (e) {
    ElMessage.error(e.message || t('common.failed'))
  } finally {
    mask.close()
    emptying.value = false
  }
}

onMounted(load)
</script>

<template>
  <div class="recycle-page">
    <header class="recycle-header">
      <div class="recycle-header-left">
        <el-button text @click="router.push('/model')">
          <i class="iconfont icon-chevron-left"></i> {{ t('common.back') }}
        </el-button>
        <span class="recycle-title">{{ t('recycle.title') }}</span>
        <span class="recycle-hint">{{ t('recycle.hint', { days: retentionDays }) }}</span>
      </div>
      <el-button
        type="danger"
        plain
        :disabled="!models.length || emptying"
        :loading="emptying"
        @click="emptyAll"
      >
        <i class="iconfont icon-trash-alt"></i> {{ t('recycle.emptyBin') }}
      </el-button>
    </header>

    <div class="recycle-body" v-loading="loading">
      <div v-if="models.length" class="recycle-list">
        <div v-for="model in models" :key="model.id" class="recycle-item">
          <div class="recycle-cover">
            <img v-if="covers.coverOf(model.id)" :src="covers.coverOf(model.id)" alt="cover" />
            <span v-else class="recycle-cover-empty">{{ t('upload.noPreview') }}</span>
          </div>
          <div class="recycle-info">
            <div class="recycle-name" :title="model.name">{{ model.name || model.fileName }}</div>
            <div class="recycle-meta">
              <span>{{ (model.fileType || '').toUpperCase() }}</span>
              <span>{{ formatSize(model.fileSize) }}</span>
              <span>{{ t('recycle.deletedAt', { date: formatDate(model.deletedAt) }) }}</span>
            </div>
          </div>
          <span class="recycle-days">{{ t('recycle.daysLeft', { days: daysLeft(model) }) }}</span>
          <div class="recycle-actions">
            <el-button size="small" plain @click="restore(model)">
              <i class="iconfont icon-repeat"></i> {{ t('recycle.restore') }}
            </el-button>
            <el-button size="small" type="danger" plain @click="remove(model)">
              {{ t('recycle.deleteForever') }}
            </el-button>
          </div>
        </div>
      </div>
      <el-empty v-else :description="t('recycle.empty')">
        <template #image>
          <i class="iconfont icon-trash-alt empty-state-icon"></i>
        </template>
      </el-empty>
    </div>
  </div>
</template>

<style scoped>
.recycle-page {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}
.recycle-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 24px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.recycle-header-left {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}
.recycle-title {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-1);
}
.recycle-hint {
  font-size: 12px;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.recycle-body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 16px 24px 28px;
}
.recycle-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.recycle-item {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 14px;
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  background: var(--bg-card);
}
.recycle-cover {
  width: 76px;
  height: 57px;
  flex-shrink: 0;
  border-radius: 6px;
  overflow: hidden;
  background: var(--bg-mute);
  display: flex;
  align-items: center;
  justify-content: center;
}
.recycle-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.recycle-cover-empty {
  font-size: 11px;
  color: var(--text-3);
}
.recycle-info {
  flex: 1;
  min-width: 0;
}
.recycle-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.recycle-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-3);
}
.recycle-days {
  flex-shrink: 0;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 12px;
  color: var(--primary);
  background: var(--primary-soft);
  border: 1px solid var(--primary);
}
.recycle-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
</style>
