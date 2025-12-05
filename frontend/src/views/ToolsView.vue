<template>
  <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
    <div class="flex flex-wrap justify-between items-center mb-8 gap-4">
      <h2 class="text-2xl font-bold text-secondary">Tool Management</h2>
      <div class="flex flex-wrap gap-4 items-center">
        <div class="flex flex-wrap gap-2 items-center">
          <select
              v-model="selectedCategory"
              @change="loadTools"
              class="select select-bordered select-sm"
          >
            <option value="">All Categories</option>
            <option v-for="category in categories" :key="category" :value="category">
              {{ formatCategory(category) }}
            </option>
          </select>

          <select
              v-model="selectedStatus"
              @change="loadTools"
              class="select select-bordered select-sm"
          >
            <option value="">All Statuses</option>
            <option v-for="status in statuses" :key="status" :value="status">
              {{ formatStatus(status) }}
            </option>
          </select>

          <input
              v-model="searchQuery"
              @input="loadTools"
              type="text"
              placeholder="Search tools..."
              class="input input-bordered input-sm w-48"
          />
        </div>

        <button
            v-if="canManageTools"
            @click="showCreateModal = true"
            class="btn btn-primary btn-sm"
        >
          Add Tool
        </button>
      </div>
    </div>

    <div v-if="loading" class="text-center py-12 text-lg">
      Loading tools...
    </div>

    <div v-else-if="error" class="alert alert-error">
      <span>{{ error }}</span>
    </div>

    <div
        v-else
        class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6 items-stretch"
    >
      <ToolCard
          :tool="tool"
          :can-manage="canManageTools"
          :can-use-based-on-training="getCanUseBasedOnTraining(tool)"
          :has-training-steps="hasTrainingSteps(tool)"
          @edit="editTool"
          @delete="deleteTool"
          @status-change="changeToolStatus"
          @view-history="viewToolHistory"
          @training-updated="onTrainingUpdated"
          @training-status-changed="onTrainingStatusChanged"
          v-for="tool in tools"
          :key="tool.id"
          class="h-full background-secondary"
      />
    </div>

    <ToolCreateModal
        v-if="showCreateModal"
        @close="showCreateModal = false"
        @created="onToolCreated"
    />

    <ToolEditModal
        v-if="editingTool"
        :tool="editingTool"
        @close="editingTool = null"
        @updated="onToolUpdated"
    />

    <ToolEventHistory
        v-if="viewingHistory"
        :tool="viewingHistory"
        @close="viewingHistory = null"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useAuthStore } from '../stores/auth'
import { toolsApi } from '../utils/api'
import { ToolCategory, ToolStatus, type Tool } from '../types/tools'
import ToolCard from '../components/ToolCard.vue'
import ToolCreateModal from '../components/ToolCreateModal.vue'
import ToolEditModal from '../components/ToolEditModal.vue'
import ToolEventHistory from '../components/ToolEventHistory.vue'

const auth = useAuthStore()

// State
const tools = ref<Tool[]>([])
const loading = ref(false)
const error = ref('')
const searchQuery = ref('')
const selectedCategory = ref<ToolCategory | ''>('')
const selectedStatus = ref<ToolStatus | ''>('')
const showCreateModal = ref(false)
const editingTool = ref<Tool | null>(null)
const viewingHistory = ref<Tool | null>(null)
const toolsWithTraining = ref<Set<string>>(new Set()) // Track which tools have training steps
const trainingOverviews = ref<Map<string, any>>(new Map()) // Cache training data

// Computed
const canManageTools = computed(() => {
  const userRole = auth.user?.role?.toLowerCase()
  return userRole === 'staff' || userRole === 'admin'
})

const currentUser = computed(() => auth.user)

const categories: ToolCategory[] = [
  ToolCategory.Saw,
  ToolCategory.PowerTool,
  ToolCategory.HandTools,
  ToolCategory.Measuring,
  ToolCategory.Safety,
  ToolCategory.Other
]

const statuses: ToolStatus[] = [
  ToolStatus.Idle,
  ToolStatus.InUse,
  ToolStatus.Maintenance,
  ToolStatus.Broken,
  ToolStatus.Repair,
  ToolStatus.Retired
]

// Methods
const loadTools = async () => {
  try {
    loading.value = true
    error.value = ''
    
    const params: Record<string, string> = {}
    if (selectedCategory.value) params.category = selectedCategory.value
    if (selectedStatus.value) params.status = selectedStatus.value
    if (searchQuery.value) params.search = searchQuery.value
    
    const response = canManageTools.value 
      ? await toolsApi.getTools(params)
      : await toolsApi.getAvailableTools?.() || await toolsApi.getTools(params)

    tools.value = response.data || []

    // Check which tools have training steps
    await checkToolsForTraining()
  } catch (err: any) {
    error.value = err.response?.data?.message || 'Failed to load tools'
  } finally {
    loading.value = false
  }
}

const checkToolsForTraining = async () => {
  // Reset training tracking
  toolsWithTraining.value.clear()

  // For each tool, check if it has training steps
  for (const tool of tools.value) {
    try {
      const response = await toolsApi.getToolTrainingSteps?.(tool.id) || { data: [] }
      if (response.data && response.data.length > 0) {
        toolsWithTraining.value.add(tool.id)
      }
    } catch (err) {
      // If training API fails, assume no training
      console.debug(`No training steps found for tool ${tool.name}`)
    }
  }
}

const editTool = (tool: Tool) => {
  editingTool.value = tool
}

const deleteTool = async (tool: Tool) => {
  if (!confirm(`Are you sure you want to delete "${tool.name}"?`)) return
  
  try {
    await toolsApi.deleteTool(tool.id)
    await loadTools()
  } catch (err: any) {
    error.value = err.response?.data?.message || 'Failed to delete tool'
  }
}

const changeToolStatus = async (tool: Tool, newStatus: ToolStatus, notes?: string) => {
  try {
    await toolsApi.updateToolStatus(tool.id, newStatus, notes)
    await loadTools()
  } catch (err: any) {
    error.value = err.response?.data?.message || 'Failed to update tool status'
  }
}

const viewToolHistory = (tool: Tool) => {
  viewingHistory.value = tool
}

const onToolCreated = () => {
  showCreateModal.value = false
  loadTools()
}

const onToolUpdated = () => {
  editingTool.value = null
  loadTools()
}

const getCanUseBasedOnTraining = (tool: Tool): boolean => {
  // If tool doesn't have training steps, user can always use it
  if (!hasTrainingSteps(tool)) return true

  // Check if we have training overview cached for this tool
  const overview = trainingOverviews.value.get(tool.id)
  return overview ? overview.can_access_tool : false
}

const hasTrainingSteps = (tool: Tool): boolean => {
  return toolsWithTraining.value.has(tool.id)
}

const onTrainingStatusChanged = (toolId: string, canAccessTool: boolean) => {
  // Update cached training status
  const existing = trainingOverviews.value.get(toolId) || {}
  trainingOverviews.value.set(toolId, { ...existing, can_access_tool: canAccessTool })
}

const onTrainingUpdated = () => {
  // Reload tools to refresh training status
  loadTools()
}

const formatCategory = (category: string) => {
  return category.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
}

const formatStatus = (status: string) => {
  return status.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
}

// Lifecycle
onMounted(() => {
  loadTools()
})
</script>

<style scoped>
.tools-view {
  padding: 2rem;
  max-width: 1400px;
  margin: 0 auto;
}

.tools-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 2rem;
  flex-wrap: wrap;
  gap: 1rem;
}

.tools-header h2 {
  margin: 0;
  color: #2c3e50;
}

.tools-controls {
  display: flex;
  gap: 1rem;
  align-items: center;
  flex-wrap: wrap;
}

.filters {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.filters select, .search-input {
  padding: 0.5rem;
  border: 1px solid #ddd;
  border-radius: 4px;
  font-size: 0.9rem;
}

.search-input {
  min-width: 200px;
}

.btn {
  padding: 0.5rem 1rem;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.9rem;
  transition: background-color 0.2s;
}

.btn-primary {
  //background-color: #3498db;
  //color: white;
}

.btn-primary:hover {
  //background-color: #2980b9;
}

.loading, .error {
  text-align: center;
  padding: 2rem;
  font-size: 1.1rem;
}

.error {
  color: #e74c3c;
  background-color: #fdf2f2;
  border: 1px solid #fbb6b6;
  border-radius: 4px;
}

.tool-item {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.tools-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(350px, 1fr));
  gap: 1.5rem;
  margin-top: 1rem;
  align-items: stretch;
}

@media (max-width: 768px) {
  .tools-header {
    flex-direction: column;
    align-items: stretch;
  }
  
  .tools-controls {
    flex-direction: column;
    align-items: stretch;
  }
  
  .filters {
    flex-wrap: wrap;
  }
  
  .tools-grid {
    grid-template-columns: 1fr;
  }
}
</style>