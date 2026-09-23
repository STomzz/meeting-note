package com.bnu.notes

import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.updatePadding

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // 边到边（Android 15+ 对 targetSdk 35+ 是强制的）：WebView 会一直画到状态栏 / 导航栏 /
    // 挖孔底下，而网页侧拿不到这些 Insets，于是设置页顶部的「推荐配置」等会被系统栏挡住。
    // 这里把系统栏的尺寸补成内容视图的内边距：整页让开系统栏，网页自己的布局不用管安全区
    // （底部同理，录音面板之类的浮动件也不会压到手势条）。
    findViewById<View?>(android.R.id.content)?.let { content ->
      ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
        val bars = insets.getInsets(
          WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
        )
        view.updatePadding(bars.left, bars.top, bars.right, bars.bottom)
        insets
      }
      ViewCompat.requestApplyInsets(content)
    }
  }
}
