<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal" @click.stop>
      <div class="modal-header">
        <h3>Edit Trainer Assignment</h3>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <div class="modal-body">
        <div class="trainer-info">
          <h4>{{ trainerWithUser.user_full_name || trainerWithUser.user_name }}</h4>
          <p class="trainer-email">{{ trainerWithUser.user_email }}</p>
        </div>

        <form @submit.prevent="submitForm">
          <div class="form-group">
            <label for="expires_at">Expiration Date</label>
            <input
              id="expires_at"
              v-model="formData.expires_at"
              type="date"
              class="form-control"
              :min="today"
            />
            <small class="form-text">Leave blank for no expiration</small>
          </div>

          <div class="form-group">
            <label for="notes">Notes</label>
            <textarea
              id="notes"
              v-model="formData.notes"
              class="form-control"
              rows="3"
              placeholder="Add any notes about this trainer assignment..."
            ></textarea>
          </div>

          <div class="form-group">
            <label class="checkbox-label">
              <input v-model="formData.is_active" type="checkbox" class="checkbox" />
              Active Trainer
            </label>
            <small class="form-text">Uncheck to temporarily deactivate this trainer</small>
          </div>

          <div v-if="error" class="error">{{ error }}</div>

          <div class="modal-actions">
            <button type="button" class="btn btn-secondary" @click="$emit('close')">Cancel</button>
            <button type="submit" :disabled="submitting" class="btn btn-primary">
              {{ submitting ? 'Updating...' : 'Update Trainer' }}
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { trainerApi } from '../utils/api'
import type { Tool, ToolTrainerWithUser, UpdateTrainerRequest } from '../types'
import { localDate, utcDateOf } from '@/lib/dates'

interface Props {
  tool: Tool
  trainerWithUser: ToolTrainerWithUser
}

const props = defineProps<Props>()
const emit = defineEmits<{
  close: []
  updated: []
}>()

// State
const submitting = ref(false)
const error = ref('')

const formData = ref<UpdateTrainerRequest & { expires_at: string }>({
  notes: props.trainerWithUser.trainer.notes || '',
  // `utcDateOf`, not `localDateOf`: this is a stored timestamp whose date
  // component is the date somebody picked. Rendering the instant locally shows
  // the previous day west of UTC, and walks back one more each time the form is
  // opened and saved.
  expires_at: utcDateOf(props.trainerWithUser.trainer.expires_at),
  is_active: props.trainerWithUser.trainer.is_active,
})

// Computed
// The user's date, not UTC's. `toISOString()` here floored the picker at the
// UTC date, so west of UTC a trainer could not be given an expiry of today.
const today = computed(() => localDate())

// Methods
const submitForm = async () => {
  try {
    submitting.value = true
    error.value = ''

    // Prepare the data
    const requestData: UpdateTrainerRequest = {
      notes: formData.value.notes || undefined,
      expires_at: formData.value.expires_at || undefined,
      is_active: formData.value.is_active,
    }

    const response = await trainerApi.updateToolTrainer(
      props.tool.id,
      props.trainerWithUser.trainer.user_id,
      requestData
    )

    if (response.success) {
      emit('updated')
    } else {
      error.value = response.error || 'Failed to update trainer'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to update trainer'
  } finally {
    submitting.value = false
  }
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal {
  background: white;
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  width: 90%;
  max-width: 500px;
  max-height: 90vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.modal-header {
  padding: 1.5rem;
  border-bottom: 1px solid #e1e5e9;
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: #f8f9fa;
}

.modal-header h3 {
  margin: 0;
  color: #2c3e50;
  font-size: 1.25rem;
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: #6c757d;
  padding: 0;
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  transition: all 0.2s;
}

.close-btn:hover {
  background: #e9ecef;
  color: #495057;
}

.modal-body {
  padding: 1.5rem;
  overflow-y: auto;
}

.trainer-info {
  background: #f8f9fa;
  padding: 1rem;
  border-radius: 6px;
  margin-bottom: 1.5rem;
}

.trainer-info h4 {
  margin: 0 0 0.25rem 0;
  color: #2c3e50;
  font-size: 1.1rem;
}

.trainer-email {
  margin: 0;
  color: #6c757d;
  font-size: 0.9rem;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group:last-child {
  margin-bottom: 0;
}

label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
  color: #2c3e50;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
}

.checkbox {
  width: auto;
  margin: 0;
}

.form-control {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 1rem;
  transition: border-color 0.2s;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.form-text {
  font-size: 0.875rem;
  color: #6c757d;
  margin-top: 0.25rem;
}

.error {
  background: #f8d7da;
  color: #721c24;
  padding: 0.75rem;
  border-radius: 4px;
  border: 1px solid #f5c6cb;
  margin-bottom: 1rem;
}

.modal-actions {
  display: flex;
  gap: 0.75rem;
  justify-content: flex-end;
  margin-top: 1.5rem;
  padding-top: 1rem;
  border-top: 1px solid #e1e5e9;
}

.btn {
  padding: 0.75rem 1.5rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 1rem;
  transition: all 0.2s;
  text-decoration: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-primary {
  background: #007bff;
  color: white;
  border-color: #007bff;
}

.btn-primary:hover:not(:disabled) {
  background: #0056b3;
  border-color: #0056b3;
}

.btn-secondary {
  background: #6c757d;
  color: white;
  border-color: #6c757d;
}

.btn-secondary:hover {
  background: #545b62;
  border-color: #545b62;
}

textarea.form-control {
  resize: vertical;
  min-height: 80px;
}

@media (max-width: 768px) {
  .modal {
    width: 95%;
    margin: 1rem;
  }

  .modal-header,
  .modal-body {
    padding: 1rem;
  }

  .modal-actions {
    flex-direction: column;
  }

  .btn {
    width: 100%;
  }
}
</style>
