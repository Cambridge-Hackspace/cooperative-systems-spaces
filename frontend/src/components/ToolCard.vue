<template>
  <div class="tool-card" :class="`status-${tool.status}`">
    <div class="tool-header">
      <div class="tool-title">
        <h3>{{ tool.name }}</h3>
        <span class="tool-category">{{ formatCategory(tool.category) }}</span>
      </div>
      <div class="tool-status">
        <span class="status-badge" :class="`status-${tool.status}`">
          {{ formatStatus(tool.status) }}
        </span>
      </div>
    </div>

    <div class="tool-info">
      <div class="info-row" v-if="tool.description">
        <strong>Description:</strong> {{ tool.description }}
      </div>
      <div class="info-row" v-if="tool.location">
        <strong>Location:</strong> {{ tool.location }}
      </div>
      <div class="info-row" v-if="tool.manufacturer">
        <strong>Manufacturer:</strong> {{ tool.manufacturer }}
      </div>
      <div class="info-row" v-if="tool.model">
        <strong>Model:</strong> {{ tool.model }}
      </div>
      <div class="info-row" v-if="tool.serial_number">
        <strong>Serial #:</strong> {{ tool.serial_number }}
      </div>
      <div class="info-row" v-if="tool.purchase_date">
        <strong>Purchased:</strong> {{ formatDate(tool.purchase_date) }}
      </div>
      <div class="info-row" v-if="tool.purchase_price">
        <strong>Price:</strong> ${{ tool.purchase_price }}
      </div>
    </div>

    <div class="tool-actions" v-if="canManage">
      <div class="status-controls">
        <select 
          :value="tool.status" 
          @change="onStatusChange"
          class="status-select"
        >
          <option value="idle">Idle</option>
          <option value="in_use">In Use</option>
          <option value="maintenance">Maintenance</option>
          <option value="broken">Broken</option>
          <option value="repair">Repair</option>
          <option value="retired">Retired</option>
        </select>
        <button 
          v-if="selectedStatus"
          @click="confirmStatusChange"
          class="btn btn-sm btn-primary"
        >
          Update
        </button>
      </div>
      
      <textarea 
        v-if="selectedStatus"
        v-model="statusChangeNotes"
        placeholder="Notes for status change..."
        class="notes-input"
        rows="2"
      ></textarea>

      <div class="action-buttons">
        <button @click="$emit('edit', tool)" class="btn btn-sm btn-secondary">
          Edit
        </button>
        <button @click="$emit('view-history', tool)" class="btn btn-sm btn-info">
          History
        </button>
        <button 
          v-if="hasTrainingSteps"
          @click="showTraining" 
          class="btn btn-sm btn-info"
        >
          <span class="training-icon">🎓</span>
          Manage Training
        </button>
        <button 
          v-else
          @click="showSetupTraining" 
          class="btn btn-sm btn-primary"
        >
          <span class="training-icon">🎓</span>
          Set Up Training
        </button>
        <button 
          @click="$emit('delete', tool)" 
          class="btn btn-sm btn-danger"
          :disabled="tool.status === 'in_use'"
        >
          Delete
        </button>
      </div>
    </div>

    <div class="member-actions" v-else>
      <div class="availability-info">
        <div v-if="tool.status === 'idle'" class="available">
          ✅ Available for use
        </div>
        <div v-else-if="tool.status === 'in_use'" class="in-use">
          ⏳ Currently in use
        </div>
        <div v-else class="unavailable">
          ❌ Not available ({{ formatStatus(tool.status) }})
        </div>
      </div>
      <!-- Training Button -->
      <button
        v-if="hasTrainingSteps"
        @click="showTraining"
        class="btn btn-info training-btn"
      >
        <span class="training-icon">🎓</span>
        View Training
      </button>

      <button
        v-if="tool.status === 'idle' && canUseBasedOnTraining"
        @click="checkCanUse"
        class="btn btn-primary"
        :disabled="loading"
      >
        {{ loading ? 'Checking...' : 'Check Out' }}
      </button>

      <div v-else-if="tool.status === 'idle' && hasTrainingSteps && !canUseBasedOnTraining" class="training-warning">
        <p>⚠️ Training required before using this tool</p>
        <small>Click "View Training" to see requirements</small>
      </div>
    </div>

    <!-- Training Modal -->
    <ToolTrainingModal
      v-if="showTrainingModal"
      :tool="tool"
      @close="showTrainingModal = false"
      @training-updated="onTrainingUpdated"
      @training-status-changed="onTrainingStatusChanged"
    />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import type { Tool, ToolStatus } from '../types/tools'
import { toolsApi } from '../utils/api'
import ToolTrainingModal from './ToolTrainingModal.vue'

interface Props {
  tool: Tool
  canManage: boolean
  canUseBasedOnTraining?: boolean
  hasTrainingSteps?: boolean
}

interface Emits {
  (e: 'edit', tool: Tool): void
  (e: 'delete', tool: Tool): void
  (e: 'status-change', tool: Tool, status: ToolStatus, notes?: string): void
  (e: 'view-history', tool: Tool): void
  (e: 'training-updated'): void
  (e: 'training-status-changed', toolId: string, canAccessTool: boolean): void
}

const props = withDefaults(defineProps<Props>(), {
  canUseBasedOnTraining: true,
  hasTrainingSteps: false
})

const emit = defineEmits<Emits>()

// State
const statusChangeNotes = ref<string | null>(null)
const selectedStatus = ref<ToolStatus | null>(null)
const loading = ref(false)
const showTrainingModal = ref(false)

// Methods
const onStatusChange = (event: Event) => {
  const target = event.target as HTMLSelectElement
  const newStatus = target.value as ToolStatus
  
  if (newStatus !== props.tool.status) {
    statusChangeNotes.value = ''
    selectedStatus.value = newStatus  // Store the selected status
  }
}

const confirmStatusChange = () => {
  if (selectedStatus.value) {
    emit('status-change', props.tool, selectedStatus.value, statusChangeNotes.value || undefined)
    statusChangeNotes.value = null
    selectedStatus.value = null
  }
}

const checkCanUse = async () => {
  try {
    loading.value = true
    const response = await toolsApi.canUseTool(props.tool.id)
    
    if (response.success && response.data) {
      if (response.data.can_use) {
        // TODO: Implement checkout flow
        alert('Tool checkout would begin here')
      } else {
        alert(`Cannot use tool: ${response.data.reason}`)
      }
    } else {
      alert('Failed to check tool availability')
    }
  } catch (err: any) {
    alert('Failed to check tool availability')
  } finally {
    loading.value = false
  }
}

const formatCategory = (category: string) => {
  return category.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
}

const formatStatus = (status: string) => {
  return status.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
}

const formatDate = (dateString: string) => {
  return new Date(dateString).toLocaleDateString()
}

const showTraining = () => {
  showTrainingModal.value = true
}

const showSetupTraining = () => {
  // For now, just show the training modal which handles both cases
  showTrainingModal.value = true
}

const onTrainingUpdated = () => {
  emit('training-updated')
}

const onTrainingStatusChanged = (toolId: string, canAccessTool: boolean) => {
  emit('training-status-changed', toolId, canAccessTool)
}
</script>

<style scoped>
.tool-card {
  background: white;
  border-radius: 8px;
  padding: 1.5rem;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  border-left: 4px solid #ddd;
  transition: all 0.2s;
}

.tool-card:hover {
  box-shadow: 0 4px 8px rgba(0, 0, 0, 0.15);
}

.tool-card.status-idle {
  border-left-color: #27ae60;
}

.tool-card.status-in_use {
  border-left-color: #f39c12;
}

.tool-card.status-maintenance {
  border-left-color: #3498db;
}

.tool-card.status-broken {
  border-left-color: #e74c3c;
}

.tool-card.status-repair {
  border-left-color: #9b59b6;
}

.tool-card.status-retired {
  border-left-color: #95a5a6;
}

.tool-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 1rem;
}

.tool-title h3 {
  margin: 0 0 0.25rem 0;
  color: #2c3e50;
  font-size: 1.2rem;
}

.tool-category {
  font-size: 0.8rem;
  color: #7f8c8d;
  text-transform: uppercase;
  font-weight: 600;
}

.status-badge {
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
}

.status-badge.status-idle {
  background-color: #d5f4e6;
  color: #27ae60;
}

.status-badge.status-in_use {
  background-color: #fdeaa7;
  color: #f39c12;
}

.status-badge.status-maintenance {
  background-color: #d6eaf8;
  color: #3498db;
}

.status-badge.status-broken {
  background-color: #fadbd8;
  color: #e74c3c;
}

.status-badge.status-repair {
  background-color: #e8daef;
  color: #9b59b6;
}

.status-badge.status-retired {
  background-color: #eaecee;
  color: #95a5a6;
}

.tool-info {
  margin-bottom: 1rem;
}

.info-row {
  margin-bottom: 0.5rem;
  font-size: 0.9rem;
  line-height: 1.4;
}

.info-row strong {
  color: #2c3e50;
}

.training-required {
  color: #e67e22;
  font-weight: 600;
}

.tool-actions, .member-actions {
  border-top: 1px solid #ecf0f1;
  padding-top: 1rem;
}

.status-controls {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  margin-bottom: 0.5rem;
}

.status-select {
  flex: 1;
  padding: 0.25rem 0.5rem;
  border: 1px solid #ddd;
  border-radius: 4px;
  font-size: 0.9rem;
}

.notes-input {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid #ddd;
  border-radius: 4px;
  margin-bottom: 0.5rem;
  font-size: 0.9rem;
  resize: vertical;
}

.action-buttons {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.btn {
  padding: 0.375rem 0.75rem;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.85rem;
  transition: background-color 0.2s;
  text-decoration: none;
  display: inline-block;
}

.btn-sm {
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}

.btn-primary {
  background-color: #3498db;
  color: white;
}

.btn-primary:hover {
  background-color: #2980b9;
}

.btn-secondary {
  background-color: #95a5a6;
  color: white;
}

.btn-secondary:hover {
  background-color: #7f8c8d;
}

.btn-info {
  background-color: #17a2b8;
  color: white;
}

.btn-info:hover {
  background-color: #138496;
}

.btn-danger {
  background-color: #e74c3c;
  color: white;
}

.btn-danger:hover {
  background-color: #c0392b;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.availability-info {
  margin-bottom: 1rem;
  padding: 0.5rem;
  border-radius: 4px;
  text-align: center;
  font-weight: 600;
}

.available {
  background-color: #d5f4e6;
  color: #27ae60;
}

.in-use {
  background-color: #fdeaa7;
  color: #f39c12;
}

.unavailable {
  background-color: #fadbd8;
  color: #e74c3c;
}

.training-warning {
  background-color: #fff3cd;
  border: 1px solid #ffeaa7;
  border-radius: 4px;
  padding: 0.75rem;
  text-align: center;
  color: #856404;
}

.training-warning p {
  margin: 0 0 0.25rem 0;
  font-weight: 600;
}

.training-warning small {
  font-size: 0.8rem;
  opacity: 0.9;
}

.training-btn {
  background-color: #17a2b8;
  color: white;
  margin-bottom: 0.5rem;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
}

.training-btn:hover {
  background-color: #138496;
}

.training-icon {
  font-size: 1rem;
}
</style>