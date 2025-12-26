/**
 * dchat Mini-App SDK
 *
 * This JavaScript SDK provides the bridge between mini-apps running in the sandbox
 * and the dchat host application. Communication happens via postMessage.
 *
 * In production, this would be injected by the sandbox runtime.
 */

(function () {
  "use strict";

  // Message types for host communication
  const MessageType = {
    // Outgoing (mini-app → host)
    READY: "dchat:ready",
    INTENT_REQUEST: "dchat:intent:request",
    WALLET_REQUEST: "dchat:wallet:request",
    STORAGE_GET: "dchat:storage:get",
    STORAGE_SET: "dchat:storage:set",
    ANALYTICS_EVENT: "dchat:analytics:event",
    UI_HAPTIC: "dchat:ui:haptic",
    UI_CLOSE: "dchat:ui:close",

    // Incoming (host → mini-app)
    INIT: "dchat:init",
    INTENT_RESPONSE: "dchat:intent:response",
    WALLET_RESPONSE: "dchat:wallet:response",
    STORAGE_RESPONSE: "dchat:storage:response",
    USER_UPDATE: "dchat:user:update",
    THEME_UPDATE: "dchat:theme:update",
  };

  // Pending request handlers
  const pendingRequests = new Map();
  let requestId = 0;

  // SDK state
  let isInitialized = false;
  let initData = null;
  let currentUser = null;
  let theme = "dark";

  // Event emitter for SDK events
  const eventHandlers = new Map();

  function emit(event, data) {
    const handlers = eventHandlers.get(event) || [];
    handlers.forEach((handler) => {
      try {
        handler(data);
      } catch (e) {
        console.error(`[dchat-sdk] Error in event handler for ${event}:`, e);
      }
    });
  }

  function on(event, handler) {
    if (!eventHandlers.has(event)) {
      eventHandlers.set(event, []);
    }
    eventHandlers.get(event).push(handler);
    return () => off(event, handler);
  }

  function off(event, handler) {
    const handlers = eventHandlers.get(event);
    if (handlers) {
      const index = handlers.indexOf(handler);
      if (index > -1) handlers.splice(index, 1);
    }
  }

  // Send message to host
  function sendToHost(type, payload = {}) {
    const message = {
      type,
      payload,
      timestamp: Date.now(),
      appId: initData?.appId || "unknown",
    };

    // In sandbox, parent is the host
    if (window.parent !== window) {
      window.parent.postMessage(message, "*");
    }

    // Also emit locally for testing/debugging
    console.debug("[dchat-sdk] →", type, payload);
  }

  // Send request and wait for response
  function sendRequest(type, responseType, payload = {}) {
    return new Promise((resolve, reject) => {
      const id = ++requestId;
      const timeoutMs = 30000;

      const timeout = setTimeout(() => {
        pendingRequests.delete(id);
        reject(new Error(`Request timeout: ${type}`));
      }, timeoutMs);

      pendingRequests.set(id, { resolve, reject, timeout, responseType });

      sendToHost(type, { ...payload, requestId: id });
    });
  }

  // Handle incoming messages from host
  function handleHostMessage(event) {
    const { data } = event;
    if (
      !data ||
      typeof data.type !== "string" ||
      !data.type.startsWith("dchat:")
    ) {
      return;
    }

    console.debug("[dchat-sdk] ←", data.type, data.payload);

    switch (data.type) {
      case MessageType.INIT:
        handleInit(data.payload);
        break;

      case MessageType.INTENT_RESPONSE:
      case MessageType.WALLET_RESPONSE:
      case MessageType.STORAGE_RESPONSE:
        handleResponse(data.type, data.payload);
        break;

      case MessageType.USER_UPDATE:
        currentUser = data.payload.user;
        emit("userUpdate", currentUser);
        break;

      case MessageType.THEME_UPDATE:
        theme = data.payload.theme;
        emit("themeUpdate", theme);
        document.documentElement.setAttribute("data-theme", theme);
        break;
    }
  }

  function handleInit(payload) {
    initData = payload;
    currentUser = payload.user;
    theme = payload.theme || "dark";
    isInitialized = true;

    document.documentElement.setAttribute("data-theme", theme);

    emit("ready", { user: currentUser, theme });
    console.info("[dchat-sdk] Initialized:", initData.appId);
  }

  function handleResponse(type, payload) {
    const { requestId, ...rest } = payload;
    const pending = pendingRequests.get(requestId);

    if (pending) {
      clearTimeout(pending.timeout);
      pendingRequests.delete(requestId);

      if (rest.error) {
        pending.reject(new Error(rest.error));
      } else {
        pending.resolve(rest);
      }
    }
  }

  // Initialize message listener
  window.addEventListener("message", handleHostMessage);

  // ========================================
  // Public SDK API
  // ========================================

  const dchat = {
    /**
     * SDK version
     */
    version: "1.0.0",

    /**
     * Check if SDK is initialized
     */
    get isReady() {
      return isInitialized;
    },

    /**
     * Get current user info
     */
    get user() {
      return currentUser;
    },

    /**
     * Get current theme
     */
    get theme() {
      return theme;
    },

    /**
     * Event handling
     */
    on,
    off,

    /**
     * Wallet API - for signing intents and transactions
     */
    wallet: {
      /**
       * Create and sign an intent
       * @param {Object} options Intent options
       * @returns {Promise<Object>} Signed intent result
       */
      async signIntent(options) {
        const { type, payload, description } = options;

        const result = await sendRequest(
          MessageType.INTENT_REQUEST,
          MessageType.INTENT_RESPONSE,
          {
            action: "sign",
            intentType: type,
            payload,
            description,
          },
        );

        return result.intent;
      },

      /**
       * Get wallet address
       * @returns {Promise<string>} User's wallet address
       */
      async getAddress() {
        const result = await sendRequest(
          MessageType.WALLET_REQUEST,
          MessageType.WALLET_RESPONSE,
          { action: "getAddress" },
        );
        return result.address;
      },

      /**
       * Request a payment
       * @param {Object} options Payment options
       * @returns {Promise<Object>} Payment result
       */
      async requestPayment(options) {
        const { amount, currency, recipient, memo } = options;

        const result = await sendRequest(
          MessageType.WALLET_REQUEST,
          MessageType.WALLET_RESPONSE,
          {
            action: "payment",
            amount,
            currency,
            recipient,
            memo,
          },
        );

        return result.transaction;
      },
    },

    /**
     * Storage API - persistent key-value storage for the mini-app
     */
    storage: {
      /**
       * Get a value from storage
       * @param {string} key Storage key
       * @returns {Promise<any>} Stored value or null
       */
      async get(key) {
        const result = await sendRequest(
          MessageType.STORAGE_GET,
          MessageType.STORAGE_RESPONSE,
          { key },
        );
        return result.value;
      },

      /**
       * Set a value in storage
       * @param {string} key Storage key
       * @param {any} value Value to store (will be JSON serialized)
       * @returns {Promise<void>}
       */
      async set(key, value) {
        await sendRequest(
          MessageType.STORAGE_SET,
          MessageType.STORAGE_RESPONSE,
          { key, value },
        );
      },

      /**
       * Remove a value from storage
       * @param {string} key Storage key
       * @returns {Promise<void>}
       */
      async remove(key) {
        await sendRequest(
          MessageType.STORAGE_SET,
          MessageType.STORAGE_RESPONSE,
          { key, value: null },
        );
      },
    },

    /**
     * UI API - interact with the host UI
     */
    ui: {
      /**
       * Trigger haptic feedback
       * @param {string} type Haptic type: 'light', 'medium', 'heavy', 'success', 'error'
       */
      haptic(type = "light") {
        sendToHost(MessageType.UI_HAPTIC, { type });
      },

      /**
       * Close the mini-app
       */
      close() {
        sendToHost(MessageType.UI_CLOSE);
      },

      /**
       * Show a toast notification
       * @param {string} message Toast message
       * @param {string} type Toast type: 'info', 'success', 'error'
       */
      showToast(message, type = "info") {
        sendToHost("dchat:ui:toast", { message, type });
      },
    },

    /**
     * Analytics API - track events
     */
    analytics: {
      /**
       * Track an event
       * @param {string} event Event name
       * @param {Object} properties Event properties
       */
      track(event, properties = {}) {
        sendToHost(MessageType.ANALYTICS_EVENT, { event, properties });
      },
    },

    /**
     * Signal that the mini-app is ready
     */
    ready() {
      sendToHost(MessageType.READY);
    },
  };

  // Expose to window
  window.dchat = dchat;

  // Auto-signal ready when DOM is loaded
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => dchat.ready());
  } else {
    dchat.ready();
  }

  console.info("[dchat-sdk] Loaded v" + dchat.version);
})();
