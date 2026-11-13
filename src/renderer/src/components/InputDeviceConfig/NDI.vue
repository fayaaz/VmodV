<template>
  <div
    v-infoView="{ title: iVTitle, body: iVBody, id: 'NDI Input Config' }"
    v-searchTerms="{
      terms: ['ndi', 'newtek', 'network', 'video'],
      title: 'NDI Config',
      type: 'Panel',
    }"
  >
    <c v-if="engineMode && engineUnavailable" span="1.." class="engine-notice">
      NDI is unavailable: libndi could not be loaded
      <template v-if="engineLibraryPath">
        (found at {{ engineLibraryPath }})
      </template>
      . Install the NDI SDK or set <code>MODV_NDI_LIB</code>, then restart the
      engine.
    </c>
    <c
      v-else-if="engineMode && engineSenderError"
      span="1.."
      class="engine-notice"
    >
      NDI output stopped: {{ engineSenderError }}
    </c>
    <c v-if="engineMode && engineNotice" span="1.." class="engine-notice">
      {{ engineNotice }}
    </c>

    <grid columns="4" class="device-config">
      <c span="4">NDI Output Settings</c>

      <c span="2">Enable Output</c>
      <c span="2">
        <Checkbox v-model="ndiOutputEnabled" class="light" />
      </c>

      <c span="2">Output Name</c>
      <c span="2"
        ><TextInput
          v-model="ndiOutputName"
          class="light"
          :disabled="ndiOutputEnabled || engineUnavailable"
          :title="
            ndiOutputEnabled
              ? 'You must disable the output to rename it'
              : undefined
          "
      /></c>

      <c span="2">Follow modV Output Resolution</c>
      <c span="2"><Checkbox v-model="followModVOutputSize" class="light" /></c>

      <c span="2">Resolution</c>
      <c span="2">
        <Number
          :disabled="followModVOutputSize"
          :model-value="followModVOutputSize ? outputWidth : ndiOutputWidth"
          @update:model="widthInput"
        />×<Number
          :disabled="followModVOutputSize"
          :model-value="followModVOutputSize ? outputHeight : ndiOutputHeight"
          @update:model="heightInput"
        />
      </c>
    </grid>
    <hr />
    <grid columns="4" class="device-config">
      <c span="3">Available NDI Sources</c>
      <c span="1">
        <Button
          class="light"
          :disabled="discovering || engineUnavailable"
          @click="discover"
        >
          {{
            discovering ? `Discovering… (${timeRemaining})` : "Discover Sources"
          }}
        </Button>
      </c>
      <c
        v-for="(source, index) in ndiSources"
        :key="index"
        span="1.."
        class="ndi-row"
      >
        <grid columns="4">
          <c span="3">{{ source.name }}</c>
          <c>
            <Button
              class="light"
              :disabled="engineUnavailable"
              @click="recieve(source)"
            >
              Recieve source
            </Button>
          </c>
        </grid>
      </c>
    </grid>

    <hr />

    <grid columns="4" class="device-config">
      <c span="1..">Available NDI Receivers</c>

      <c
        v-for="receiver in ndiReceivers"
        :key="receiver.id"
        span="1.."
        class="ndi-row"
      >
        <grid columns="4">
          <c span="2">
            {{ receiver.receiver.source.name }}
            <span v-if="receiver.state" class="receiver-state">
              ({{ receiver.state
              }}<template v-if="receiver.width">
                {{ receiver.width }}×{{ receiver.height }}</template
              >)
            </span>
            <div v-if="receiver.error" class="receiver-error">
              {{ receiver.error }}
            </div>
          </c>
          <c>
            <Button class="light" @click="toggleReceiver(receiver)">
              {{ receiver.enabled ? "Disable" : "Enable" }}
            </Button>
          </c>
          <c>
            <Button class="light" @click="removeReceiver(receiver)">
              Remove receiver
            </Button>
          </c>
        </grid>
      </c>
    </grid>
  </div>
</template>

<script>
import Checkbox from "../inputs/Checkbox.vue";

// How often the engine-mode panel refreshes the engine's NDI status (sender
// errors, availability). The mirror already carries receiver updates, so this
// only covers the sender/library state.
const ENGINE_STATUS_INTERVAL_MS = 2000;

export default {
  components: {
    Checkbox,
  },

  data() {
    return {
      iVTitle: "NDI Config",
      iVBody:
        "Configure your NDI settings here. Click Discover Sources to search for NDI sources available on your network. Click Enable to create an NDI reciever so modV can consume the NDI source. Once a receiver has been created, click Remove Receiver to stop modV consuming the NDI source.",

      timeRemaining: 0,
      timer: null,
      // Engine mode: the engine owns libndi, so the panel has to ask it
      // whether the SDK loaded. `null` means "not asked yet".
      engineAvailable: null,
      engineLibraryPath: null,
      engineSenderError: null,
      engineNotice: "",
      engineStatusTimer: null,
    };
  },

  computed: {
    engineMode() {
      return this.$modV?.engineMode === true;
    },

    engineUnavailable() {
      return this.engineMode && this.engineAvailable === false;
    },

    discovering() {
      return this.$modV.store.state.ndi.discovering;
    },

    ndiSources() {
      return this.$modV.store.state.ndi.sources;
    },

    ndiReceivers() {
      return this.$modV.store.state.ndi.receivers;
    },

    outputWidth() {
      return this.$modV.store.state.size.width;
    },

    outputHeight() {
      return this.$modV.store.state.size.height;
    },

    ndiOutputEnabled: {
      get() {
        return this.$modV.store.state.ndi.outputEnabled;
      },

      set(value) {
        // The engine cannot start a sender without libndi; keep the mirror
        // (and the checkbox) off instead of showing a failed output.
        if (value && this.engineUnavailable) {
          return;
        }
        this.$modV.store.commit("ndi/SET_OUTPUT_ENABLED", value);
      },
    },

    ndiOutputName: {
      get() {
        return this.$modV.store.state.ndi.outputName;
      },
      set(value) {
        this.$modV.store.commit("ndi/SET_OUTPUT_NAME", value);
      },
    },

    ndiOutputWidth() {
      return this.$modV.store.state.ndi.outputWidth;
    },

    ndiOutputHeight() {
      return this.$modV.store.state.ndi.outputHeight;
    },

    followModVOutputSize: {
      get() {
        return this.$modV.store.state.ndi.followModVOutputSize;
      },
      set(value) {
        this.$modV.store.commit("ndi/SET_FOLLOW_MODV_OUTPUT_SIZE", value);
      },
    },
  },

  watch: {
    discovering(value) {
      if (!value) {
        clearInterval(this.timer);
      }
    },
  },

  mounted() {
    if (!this.engineMode) {
      return;
    }

    void this.refreshEngineStatus();
    this.engineStatusTimer = setInterval(() => {
      void this.refreshEngineStatus();
    }, ENGINE_STATUS_INTERVAL_MS);
  },

  beforeUnmount() {
    clearInterval(this.timer);
    clearInterval(this.engineStatusTimer);
  },

  methods: {
    /** Ask the engine for libndi availability and the sender's status. */
    async refreshEngineStatus() {
      try {
        const status = await this.$modV.store.dispatch("ndi/getState", {});
        if (!status || typeof status !== "object") {
          return;
        }

        this.engineAvailable = status.available === true;
        this.engineLibraryPath = status.libraryPath ?? null;
        this.engineSenderError = status.sender?.error ?? null;
      } catch (error) {
        // The engine may still be starting; the next tick retries.
        this.engineAvailable = null;
      }
    },

    async discover() {
      this.engineNotice = "";

      // The timer starts immediately: in legacy mode the worker's discovery
      // runs for the full timeout, and in engine mode the mirror receives
      // `ndi/SET_DISCOVERING` from the engine poll.
      this.timeRemaining = this.$modV.store.state.ndi.timeout / 1000;
      this.timer = setInterval(() => {
        this.timeRemaining -= 1;

        if (this.timeRemaining < 1) {
          clearInterval(this.timer);
        }
      }, 1000);

      try {
        await this.$modV.store.dispatch("ndi/discoverSources");
      } catch (error) {
        clearInterval(this.timer);
        this.engineNotice = error?.message || String(error);
      }
    },

    async recieve(source) {
      this.engineNotice = "";

      try {
        await this.$modV.store.dispatch("ndi/createReceiver", {
          source: JSON.parse(JSON.stringify(source)),
        });
      } catch (error) {
        this.engineNotice = error?.message || String(error);
      }
    },

    toggleReceiver(receiver) {
      const action = receiver.enabled
        ? "ndi/disableReceiver"
        : "ndi/enableReceiver";

      this.$modV.store
        .dispatch(action, { receiverId: receiver.id })
        .catch((error) => {
          this.engineNotice = error?.message || String(error);
        });
    },

    removeReceiver(receiver) {
      this.$modV.store
        .dispatch("ndi/removeReceiver", { receiverId: receiver.id })
        .catch((error) => {
          this.engineNotice = error?.message || String(error);
        });
    },

    widthInput(width) {
      this.$modV.store.commit("ndi/SET_OUTPUT_SIZE", {
        width,
      });
    },

    heightInput(height) {
      this.$modV.store.commit("ndi/SET_OUTPUT_SIZE", {
        height,
      });
    },
  },
};
</script>

<style>
.ndi-row:nth-child(odd) {
  background-color: var(--foreground-color-4);
}
.ndi-row:nth-child(even) {
  background-color: var(--foreground-color-3);
}
.engine-notice {
  color: var(--foreground-color-1);
  background-color: var(--foreground-color-4);
  padding: 0.4rem 0.6rem;
  border-radius: 0.25rem;
}
.receiver-state {
  opacity: 0.7;
}
.receiver-error {
  color: var(--red);
  font-size: 0.85em;
}
</style>
