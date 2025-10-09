<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <h3>{{ tool.name }} - Event History</h3>
        <button @click="$emit('close')" class="close-btn">&times;</button>
      </div>

      <div class="modal-body">
        <div class="loading" v-if="loading">Loading events...</div>
        
        <div class="error" v-else-if="error">
          {{ error }}
        </div>

        <div v-else-if="events.length === 0" class="no-events">
          No events recorded for this tool.
        </div>

        <div v-else class="events-list">
          <div 
            v-for="event in events" 
            :key="event.id" 
            class="event-item"
            :class="`event-${event.event_type}`"
          >
            <div class="event-header">
              <div class="event-type">
                <span class="event-icon">{{ getEventIcon(event.event_type) }}</span>
                {{ formatEventType(event.event_type) }}
              </div>
              <div class="event-date">
                {{ formatDate(event.created_at) }}
              </div>
            </div>

            <div class="event-details">
              <div v-if="event.user_username" class="event-user">
                By: {{ event.user_username }}
              </div>
              
              <div v-if="event.old_status || event.new_status" class="status-change">
                <span v-if="event.old_status" class="old-status">
                  {{ formatStatus(event.old_status) }}
                </span>
                <span v-if="event.old_status && event.new_status" class="arrow">→</span>
                <span v-if="event.new_status" class="new-status">
                  {{ formatStatus(event.new_status) }}
                </span>
              </div>

              <div v-if="event.metadata" class="event-metadata">
                <div v-if="event.metadata.scan_data" class="scan-data">
                  Scan: {{ event.metadata.scan_data }}
                </div>
                <div v-if="event.metadata.checkout_duration" class="checkout-duration">
                  Duration: {{ event.metadata.checkout_duration }}
                </div>
              </div>

              <div v-if="event.notes" class="event-notes">
                <strong>Notes:</strong> {{ event.notes }}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="modal-actions">
        <button @click="$emit('close')" class="btn btn-secondary">
          Close
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { toolsApi } from '../utils/api'
import type { Tool, ToolEvent } from '../types/tools'

interface Props {
  tool: Tool
}

interface Emits {
  (e: 'close'): void
}

const props = defineProps<Props>()
const emit = defineEmits<Emits>()

// State
const events = ref<ToolEvent[]>([])
const loading = ref(false)
const error = ref('')

// Methods
const loadEvents = async () => {
  try {
    loading.value = true
    error.value = ''
    
    const response = await toolsApi.getToolEvents(props.tool.id)
    events.value = response.data
  } catch (err: any) {
    error.value = err.response?.data?.message || 'Failed to load tool events'
  } finally {
    loading.value = false
  }
}

const formatEventType = (eventType: string) => {
  return eventType.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
}

const formatStatus = (status: string) => {
  return status.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
}

const formatDate = (dateString: string) => {
  const date = new Date(dateString)
  return date.toLocaleString()
}

const getEventIcon = (eventType: string) => {
  const icons: Record<string, string> = {
    'created': '🆕',
    'status_change': '🔄',
    'maintenance': '🔧',
    'scan': '📱',
    'checkout': '📤',
    'return': '📥',
    'deleted': '🗑️'
  }
  return icons[eventType] || '📝'
}

// Lifecycle
onMounted(() => {
  loadEvents()
})
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: rgba(0, 0, 0, 0.5);
  display: flex;
  justify-content: center;
  align-items: center;
  z-index: 1000;
}

.modal-content {
  background: white;
  border-radius: 8px;
  width: 90%;
  max-width: 700px;
  max-height: 90vh;
  display: flex;
  flex-direction: column;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem;
  border-bottom: 1px solid #ecf0f1;
  flex-shrink: 0;
}

.modal-header h3 {
  margin: 0;
  color: #2c3e50;
}

.close-btn {
  background: none;
  border: none;
  font-size: 2rem;
  color: #95a5a6;
  cursor: pointer;
  padding: 0;
  width: 2rem;
  height: 2rem;
  display: flex;
  align-items: center;
  justify-content: center;
}

.close-btn:hover {
  color: #7f8c8d;
}

.modal-body {
  flex: 1;
  overflow-y: auto;
  padding: 1.5rem;
}

.loading {
  text-align: center;
  padding: 2rem;
  color: #7f8c8d;
}

.error {
  color: #e74c3c;
  background-color: #fdf2f2;
  border: 1px solid #fbb6b6;
  border-radius: 4px;
  padding: 1rem;
  text-align: center;
}

.no-events {
  text-align: center;
  padding: 2rem;
  color: #7f8c8d;
}

.events-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.event-item {
  background: #f8f9fa;
  border: 1px solid #e9ecef;
  border-radius: 8px;
  padding: 1rem;
  position: relative;
}

.event-item.event-created {
  border-left: 4px solid #27ae60;
}

.event-item.event-status_change {
  border-left: 4px solid #3498db;
}

.event-item.event-maintenance {
  border-left: 4px solid #f39c12;
}

.event-item.event-scan {
  border-left: 4px solid #9b59b6;
}

.event-item.event-checkout {
  border-left: 4px solid #e67e22;
}

.event-item.event-return {
  border-left: 4px solid #27ae60;
}

.event-item.event-deleted {
  border-left: 4px solid #e74c3c;
}

.event-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.5rem;
}

.event-type {
  display: flex;
  align-items: center;
  font-weight: 600;
  color: #2c3e50;
}

.event-icon {
  margin-right: 0.5rem;
}

.event-date {
  color: #7f8c8d;
  font-size: 0.9rem;
}

.event-details {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.event-user {
  color: #7f8c8d;
  font-size: 0.9rem;
}

.status-change {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.9rem;
}

.old-status {
  background: #ecf0f1;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  color: #7f8c8d;
}

.new-status {
  background: #d5f4e6;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  color: #27ae60;
}

.arrow {
  color: #7f8c8d;
}

.event-metadata {
  font-size: 0.9rem;
  color: #7f8c8d;
}

.event-notes {
  background: white;
  padding: 0.75rem;
  border-radius: 4px;
  border: 1px solid #e9ecef;
  font-size: 0.9rem;
  margin-top: 0.5rem;
}

.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 1rem;
  padding: 1.5rem;
  border-top: 1px solid #ecf0f1;
  flex-shrink: 0;
}

.btn {
  padding: 0.75rem 1.5rem;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.9rem;
  transition: background-color 0.2s;
}

.btn-secondary {
  background-color: #95a5a6;
  color: white;
}

.btn-secondary:hover {
  background-color: #7f8c8d;
}

@media (max-width: 768px) {
  .modal-content {
    width: 95%;
    margin: 1rem;
  }
  
  .event-header {
    flex-direction: column;
    align-items: flex-start;
    gap: 0.25rem;
  }
  
  .status-change {
    flex-wrap: wrap;
  }
}
</style>