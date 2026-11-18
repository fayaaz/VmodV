<template>
  <grid columns="4">
    <c span="1..">
      <grid columns="4">
        <c span="1"> Audio Feature </c>
        <c span="2">
          <Select
            v-model="feature"
            class="light"
            @update:model-value="checkFeature"
          >
            <option
              v-for="featureValue in features"
              :key="featureValue"
              :value="featureValue"
            >
              {{ featureValue }}
            </option>
          </Select>
        </c>
        <c span="1"> ({{ currentFeatureValue }}) </c>
      </grid>
    </c>

    <c v-if="feature !== bpmFeature" span="1..">
      <grid columns="4">
        <c span="1">Smoothing</c>
        <c span="3">
          <RangeControl
            :min="0"
            :max="MAX_SMOOTHING - SMOOTHING_STEP"
            :step="SMOOTHING_STEP"
            :model-value="invertedInputValue"
            @update:model-value="smoothingInput"
          />
        </c>
      </grid>
    </c>
  </grid>
</template>

<script>
import RangeControl from "../Controls/RangeControl.vue";
import {
  BPM_LINK_SOURCE,
  createBpmInputLink,
} from "../../application/worker/bpm-link";

export default {
  components: {
    RangeControl,
  },
  props: {
    inputId: {
      type: String,
      required: true,
    },
  },

  data() {
    return {
      iVTitle: "Audio Feature Smoothing",
      iVBody:
        "Turning on audio feature smoothing gives the audio feature a smoother decent to the set minimum value when linked to module properties. Smoothing captures the last peak value of the selected audio feature and attempts to slowly decrease the value back down to the minumum value. If the peak value is greater than the current captured peak the new peak will take priority.",
      iVID: "Audio Feature Smoothing",

      features: [
        "none",
        "bpm",
        "kick",
        "bass",
        "mid",
        "treble",
        "rms",
        "zcr",
        "energy",
        "spectralCentroid",
        "spectralFlatness",
        "spectralSlope",
        "spectralRolloff",
        "spectralSpread",
        "spectralSkewness",
        "spectralKurtosis",
        "perceptualSpread",
        "perceptualSharpness",
      ],

      feature: "none",
      smoothingId: null,
      smoothingValue: 0,
    };
  },

  computed: {
    bpmFeature() {
      return BPM_LINK_SOURCE;
    },

    currentFeatureValue() {
      if (this.feature === BPM_LINK_SOURCE) {
        return Math.round(this.$modV.store.state.beats.bpm);
      }

      if (this.feature === "kick") {
        return Number(this.$modV.store.state.beats.kick);
      }

      const value = this.$modV.features[this.feature];

      return value === undefined ? "" : value.toFixed(2);
    },

    MAX_SMOOTHING() {
      return this.$modV.store.state.meyda.MAX_SMOOTHING;
    },

    SMOOTHING_STEP() {
      return this.$modV.store.state.meyda.SMOOTHING_STEP;
    },

    inputLink() {
      return this.$modV.store.state.inputs.inputLinks[this.inputId];
    },

    invertedInputValue() {
      return this.MAX_SMOOTHING - this.smoothingValue;
    },
  },

  watch: {
    async smoothingValue(value) {
      if (value && !this.smoothingId) {
        this.smoothingId = await this.$modV.store.dispatch(
          "meyda/getSmoothingId",
        );

        this.updateInputLinkArgs([
          this.feature,
          this.smoothingId,
          this.smoothingValue,
        ]);
      } else if (value && this.smoothingId) {
        this.updateInputLinkArgs([
          this.feature,
          this.smoothingId,
          this.smoothingValue,
        ]);
      } else if (!value && this.smoothingId) {
        // Capture the id first: `meyda/removeSmoothingId` takes the id as its
        // payload, so clearing the field before dispatching handed the engine
        // `null` and the smoothing entry leaked.
        const smoothingId = this.smoothingId;
        this.smoothingId = null;
        this.$modV.store.dispatch("meyda/removeSmoothingId", smoothingId);
        this.updateInputLinkArgs([this.feature]);
      }
    },

    inputId() {
      this.restoreLinkValues();

      if (
        !this.inputLink ||
        (this.inputLink.source !== "meyda" &&
          this.inputLink.source !== BPM_LINK_SOURCE)
      ) {
        this.feature = "none";
        this.smoothingId = null;
        this.smoothingValue = this.MAX_SMOOTHING - this.SMOOTHING_STEP;
      }
    },
  },

  created() {
    this.restoreLinkValues();
  },

  methods: {
    restoreLinkValues() {
      const link = this.inputLink;

      if (link && link.source === BPM_LINK_SOURCE) {
        this.feature = BPM_LINK_SOURCE;
        this.smoothingId = null;
        this.smoothingValue = 0;

        return;
      }

      if (link && link.source === "meyda") {
        if (link.args.length > 2) {
          this.smoothingValue = link.args[2];
          // The id was minted when the link was created; keep it so turning
          // smoothing back down releases it instead of leaking the entry.
          this.smoothingId = link.args[1] ?? null;
        }

        this.feature = link.args[0];
      }
    },

    makeLink() {
      if (this.feature === BPM_LINK_SOURCE) {
        // A BPM link has no smoothing args, so release a smoothing entry the
        // previous audio link minted instead of leaking it.
        if (this.smoothingId) {
          this.$modV.store.dispatch(
            "meyda/removeSmoothingId",
            this.smoothingId,
          );
          this.smoothingId = null;
          this.smoothingValue = 0;
        }

        this.$modV.store.dispatch(
          "inputs/createInputLink",
          createBpmInputLink(this.inputId),
        );
      } else if (this.feature === "kick") {
        this.$modV.store.dispatch("inputs/createInputLink", {
          inputId: this.inputId,
          type: "mutation",
          location: "beats.kick",
          match: {
            type: "beats/SET_KICK",
          },
          source: "meyda",
          args: [this.feature],
        });
      } else {
        this.$modV.store.dispatch("inputs/createInputLink", {
          inputId: this.inputId,
          type: "getter",
          location: "meyda/getFeature",
          source: "meyda",
          args: [this.feature],
        });
      }
    },

    removeLink() {
      this.$modV.store.dispatch("inputs/removeInputLink", {
        inputId: this.inputId,
      });
    },

    smoothingInput(value) {
      this.smoothingValue = this.MAX_SMOOTHING - value;
    },

    updateInputLinkArgs(value) {
      this.$modV.store.dispatch("inputs/updateInputLink", {
        inputId: this.inputId,
        key: "args",
        value: value,
      });
    },

    checkFeature() {
      if (
        this.feature === "none" &&
        this.inputLink &&
        (this.inputLink.source === "meyda" ||
          this.inputLink.source === BPM_LINK_SOURCE)
      ) {
        this.removeLink();
      } else {
        this.makeLink();
      }
    },
  },
};
</script>
