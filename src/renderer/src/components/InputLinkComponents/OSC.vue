<template>
  <grid columns="4">
    <c span="1..">
      <grid columns="4">
        <c span="2+2">
          <Textarea v-model="oscLocation" @change="updateLocation" />
        </c>
      </grid>
    </c>

    <c span="1..">
      <grid columns="4">
        <c span="1">Listener</c>
        <c span="3">
          <Button v-if="!listening" class="light" @click="startListening">
            Listen on {{ host }}:{{ port }}
          </Button>
          <Button v-else class="light" @click="stopListening">
            Listening on {{ serverLabel }} — stop
          </Button>
        </c>
      </grid>
    </c>
  </grid>
</template>

<script>
export default {
  props: {
    inputId: {
      type: String,
      required: true,
    },
  },

  data() {
    return {
      oscLocation: "value",
      host: "0.0.0.0",
      port: 3333,
    };
  },

  // watch: {
  //   inputId(inputId) {
  //     this.restoreExpressionValues(inputId);
  //   },
  // },

  // created() {
  //   this.restoreExpressionValues();
  // },

  computed: {
    inputConfig() {
      return this.$modV.store.state.inputs.inputLinks[this.inputId] || {};
    },

    hasLink() {
      const link = this.$modV.store.state.inputs.inputLinks[this.inputId];

      return link && link.source === "osc";
    },

    servers() {
      return this.$modV.store.state.osc?.servers || {};
    },

    listening() {
      return Object.keys(this.servers).length > 0;
    },

    serverLabel() {
      const [server] = Object.values(this.servers);

      return server?.name || `${this.host}:${this.port}`;
    },
  },

  methods: {
    async startListening() {
      await this.$modV.store.dispatch("osc/createServer", {
        host: this.host,
        port: this.port,
      });
    },

    async stopListening() {
      for (const id of Object.keys(this.servers)) {
        await this.$modV.store.dispatch("osc/removeServer", { id });
      }
    },

    async updateLocation() {
      const { oscLocation } = this;

      if (oscLocation.length === 0) {
        this.$modV.store.dispatch("inputs/removeInputLink", {
          inputId: this.inputId,
        });

        return;
      }

      await this.$modV.store.dispatch("inputs/createInputLink", {
        inputId: this.inputId,
        type: "state",
        source: "osc",
        location: oscLocation,
        min: null,
        max: null,
      });
    },
  },
};
</script>
