<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content bg-base-100" @click.stop>
      <div class="modal-header bg-gradient-to-br from-primary via-secondary to-primary">
        <h3>Create New Tool</h3>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <form class="tool-form" @submit.prevent="createTool">
        <div class="form-row">
          <div class="form-group">
            <label for="name">Name *</label>
            <input
              id="name"
              v-model="form.name"
              type="text"
              required
              placeholder="Tool name"
              class="input"
            />
          </div>

          <div class="form-group">
            <label for="category">Category *</label>
            <select id="category" v-model="form.category" class="select" required>
              <option value="">Select Category</option>
              <option value="saw">Saw</option>
              <option value="powertool">Power Tool</option>
              <option value="hand_tools">Hand Tools</option>
              <option value="measuring">Measuring</option>
              <option value="safety">Safety</option>
              <option value="other">Other</option>
            </select>
          </div>
        </div>

        <div class="form-group">
          <label for="description">Description</label>
          <textarea
            id="description"
            v-model="form.description"
            placeholder="Brief description of the tool"
            rows="3"
            class="textarea"
          ></textarea>
        </div>

        <div class="form-row">
          <div class="form-group">
            <label for="manufacturer">Manufacturer</label>
            <input
              id="manufacturer"
              v-model="form.manufacturer"
              type="text"
              placeholder="e.g., DeWalt, Milwaukee"
              class="input"
            />
          </div>

          <div class="form-group">
            <label for="model">Model</label>
            <input
              id="model"
              v-model="form.model"
              type="text"
              placeholder="Model number"
              class="input"
            />
          </div>
        </div>

        <div class="form-row">
          <div class="form-group">
            <label for="serial_number">Serial Number</label>
            <input
              id="serial_number"
              v-model="form.serial_number"
              type="text"
              placeholder="Serial number"
              class="input"
            />
          </div>

          <div class="form-group">
            <label for="barcode">Barcode</label>
            <input
              id="barcode"
              v-model="form.barcode"
              type="text"
              placeholder="Barcode or QR code"
              class="input"
            />
          </div>
        </div>

        <div class="form-row">
          <div class="form-group">
            <label for="location">Location</label>
            <input
              id="location"
              v-model="form.location"
              type="text"
              placeholder="Where the tool is stored"
              class="input"
            />
          </div>

          <div class="form-group">
            <label for="schedule_id">Usability schedule</label>
            <SchedulePicker
              :model-value="form.schedule_id ?? null"
              :schedules="schedules"
              @update:model-value="(v) => (form.schedule_id = v ?? undefined)"
            />
            <small class="help-text">Optional — restrict when the tool can be used.</small>
          </div>

          <div class="form-group">
            <label for="status">Initial Status</label>
            <select id="status" v-model="form.status" class="select">
              <option value="idle">Idle</option>
              <option value="maintenance">Maintenance</option>
              <option value="broken">Broken</option>
              <option value="repair">Repair</option>
            </select>
          </div>
        </div>

        <div class="form-row">
          <div class="form-group">
            <label for="purchase_date">Purchase Date</label>
            <input id="purchase_date" v-model="form.purchase_date" type="date" class="input" />
          </div>

          <div class="form-group">
            <label for="purchase_price">Purchase Price</label>
            <input
              id="purchase_price"
              v-model.number="form.purchase_price"
              type="number"
              step="0.01"
              placeholder="0.00"
              class="input"
            />
          </div>
        </div>

        <div class="form-group">
          <label class="checkbox-label">
            <input v-model="form.requires_training" type="checkbox" class="checkbox" />
            Requires Training
          </label>
        </div>

        <div class="form-group">
          <label for="notes">Notes</label>
          <textarea
            id="notes"
            v-model="form.notes"
            placeholder="Additional notes about the tool"
            rows="3"
            class="textarea"
          ></textarea>
        </div>

        <div v-if="error" class="error">
          {{ error }}
        </div>

        <div class="modal-actions">
          <button type="button" class="btn btn-secondary" @click="$emit('close')">Cancel</button>
          <button type="submit" class="btn btn-primary" :disabled="loading">
            {{ loading ? 'Creating...' : 'Create Tool' }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { toolsApi, schedulesApi } from '../utils/api'
import type { NewTool, ToolCategory } from '../types/tools'
import type { Schedule } from '../types'
import SchedulePicker from './SchedulePicker.vue'

interface Emits {
  (e: 'close'): void
  (e: 'created'): void
}

const emit = defineEmits<Emits>()

// State
const loading = ref(false)
const error = ref('')
const schedules = ref<Schedule[]>([])

const form = ref<NewTool & { schedule_id?: string | null }>({
  name: '',
  category: '' as ToolCategory,
  description: '',
  manufacturer: '',
  model: '',
  serial_number: '',
  barcode: '',
  location: '',
  status: 'idle',
  purchase_date: '',
  purchase_price: null,
  requires_training: false,
  notes: '',
  schedule_id: null,
})

const loadSchedules = async () => {
  try {
    const r = await schedulesApi.list()
    if (r.success && r.data) schedules.value = r.data
  } catch {
    schedules.value = []
  }
}

// Methods
const createTool = async () => {
  try {
    loading.value = true
    error.value = ''

    // Clean up form data
    const toolData = { ...form.value }

    // Convert empty strings to null for optional fields
    Object.keys(toolData).forEach((key) => {
      if (toolData[key as keyof NewTool] === '') {
        ;(toolData as any)[key] = null
      }
    })

    const response = await toolsApi.createTool(toolData)

    // Read the flag. `createTool` catches its own rejection and resolves with
    // `{ success: false }`, so an unchecked `await` announced every refusal as
    // a success: `created` emitted, the parent refreshing a list that had not
    // changed, and nothing shown.
    if (!response.success) {
      error.value = response.error || 'Failed to create tool'
      return
    }
    emit('created')
  } catch (err: any) {
    error.value = err.response?.data?.error || err.message || 'Failed to create tool'
  } finally {
    loading.value = false
  }
}

onMounted(loadSchedules)
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
  border-radius: 8px;
  width: 90%;
  max-width: 600px;
  max-height: 90vh;
  overflow-y: auto;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem;
  border-bottom: 1px solid #ecf0f1;
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

.tool-form {
  padding: 1.5rem;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1rem;
  margin-bottom: 1rem;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 600;
}

.checkbox-label {
  display: flex;
  align-items: center;
  cursor: pointer;
}

.checkbox-label input {
  margin-right: 0.5rem;
}

.form-group input,
.form-group select,
.form-group textarea {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid #ddd;
  border-radius: 4px;
  font-size: 0.9rem;
  transition: border-color 0.2s;
}

.form-group input:focus,
.form-group select:focus,
.form-group textarea:focus {
  outline: none;
  border-color: #3498db;
  box-shadow: 0 0 0 2px rgba(52, 152, 219, 0.1);
}

.form-group textarea {
  resize: vertical;
}

.error {
  color: #e74c3c;
  background-color: #fdf2f2;
  border: 1px solid #fbb6b6;
  border-radius: 4px;
  padding: 0.75rem;
  margin-bottom: 1rem;
}

.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 1rem;
  padding-top: 1rem;
  border-top: 1px solid #ecf0f1;
}

.btn {
  padding: 0.75rem 1.5rem;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.9rem;
  transition: background-color 0.2s;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

@media (max-width: 768px) {
  .modal-content {
    width: 95%;
    margin: 1rem;
  }

  .form-row {
    grid-template-columns: 1fr;
  }

  .modal-actions {
    flex-direction: column;
  }
}
</style>
