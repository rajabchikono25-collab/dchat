/**
 * dchat Android Bridge Application
 * Initializes native bridges on application startup
 */
package com.dchat.bridge

import android.app.Application
import android.app.Activity
import android.os.Bundle
import androidx.fragment.app.FragmentActivity

/**
 * Application class that initializes the native bridges
 */
open class DchatBridgeApplication : Application() {
    
    override fun onCreate() {
        super.onCreate()
        
        // Initialize bridges with application context
        BiometricBridge.initialize(this)
        EnclaveBridge.initialize(this)
        
        // Register activity lifecycle callbacks to track current activity
        registerActivityLifecycleCallbacks(object : ActivityLifecycleCallbacks {
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
            
            override fun onActivityStarted(activity: Activity) {}
            
            override fun onActivityResumed(activity: Activity) {
                if (activity is FragmentActivity) {
                    BiometricBridge.setCurrentActivity(activity)
                }
            }
            
            override fun onActivityPaused(activity: Activity) {
                if (activity is FragmentActivity) {
                    BiometricBridge.setCurrentActivity(null)
                }
            }
            
            override fun onActivityStopped(activity: Activity) {}
            
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
            
            override fun onActivityDestroyed(activity: Activity) {}
        })
    }
}

/**
 * Static initialization helper for apps that can't extend DchatBridgeApplication
 */
object DchatBridgeInit {
    
    private var initialized = false
    
    /**
     * Initialize the native bridges manually
     * Call this from your Application.onCreate() if you can't extend DchatBridgeApplication
     */
    @JvmStatic
    @Synchronized
    fun initialize(application: Application) {
        if (initialized) return
        
        BiometricBridge.initialize(application)
        EnclaveBridge.initialize(application)
        
        application.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
            override fun onActivityStarted(activity: Activity) {}
            
            override fun onActivityResumed(activity: Activity) {
                if (activity is FragmentActivity) {
                    BiometricBridge.setCurrentActivity(activity)
                }
            }
            
            override fun onActivityPaused(activity: Activity) {
                if (activity is FragmentActivity) {
                    BiometricBridge.setCurrentActivity(null)
                }
            }
            
            override fun onActivityStopped(activity: Activity) {}
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
            override fun onActivityDestroyed(activity: Activity) {}
        })
        
        initialized = true
    }
}
