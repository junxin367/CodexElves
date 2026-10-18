package com.junxin367.codexelves;

import android.content.Context;
import android.text.Editable;
import android.text.TextWatcher;
import android.text.method.PasswordTransformationMethod;
import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ImageButton;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

final class LoginView extends ScrollView {
    final EditText originInput;
    final EditText keyInput;
    final TextView status;
    private final Button connect;
    private final ImageButton keyVisibility;
    private final LinearLayout panel;
    private boolean busy;
    private boolean keyVisible;
    private Runnable submit;

    public LoginView(Context context) {
        super(context);
        setFillViewport(true);
        setClipToPadding(false);
        setBackgroundColor(context.getColor(R.color.login_background));
        View.inflate(context, R.layout.view_login, this);
        panel = findViewById(R.id.login_panel);
        originInput = findViewById(R.id.login_origin);
        keyInput = findViewById(R.id.login_key);
        keyVisibility = findViewById(R.id.login_key_visibility);
        connect = findViewById(R.id.login_connect);
        status = findViewById(R.id.login_status);

        TextWatcher inputChanges = new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence value, int start, int count, int after) {}

            @Override
            public void onTextChanged(CharSequence value, int start, int before, int count) {
                if (!busy) {
                    status.setText("");
                    status.setVisibility(View.INVISIBLE);
                }
                updateControls();
            }

            @Override
            public void afterTextChanged(Editable value) {}
        };
        originInput.addTextChangedListener(inputChanges);
        keyInput.addTextChangedListener(inputChanges);
        keyVisibility.setOnClickListener(ignored -> setKeyVisible(!keyVisible));
        connect.setOnClickListener(ignored -> submit());
        keyInput.setOnEditorActionListener((view, actionId, event) -> {
            boolean enterPressed = event != null
                && event.getKeyCode() == KeyEvent.KEYCODE_ENTER
                && event.getAction() == KeyEvent.ACTION_DOWN;
            if (actionId == EditorInfo.IME_ACTION_GO
                || actionId == EditorInfo.IME_ACTION_DONE || enterPressed) {
                submit();
                return true;
            }
            return false;
        });
        keyInput.setTransformationMethod(PasswordTransformationMethod.getInstance());

        View helpToggle = findViewById(R.id.login_help_toggle);
        TextView helpBody = findViewById(R.id.login_help_body);
        ImageView helpChevron = findViewById(R.id.login_help_chevron);
        helpToggle.setOnClickListener(ignored -> {
            boolean expanded = helpBody.getVisibility() != View.VISIBLE;
            helpBody.setVisibility(expanded ? View.VISIBLE : View.GONE);
            helpChevron.setRotation(expanded ? 180 : 0);
            helpToggle.setContentDescription(context.getString(
                expanded ? R.string.login_help_collapse : R.string.login_help_expand
            ));
        });
        updateControls();
    }

    void setInitialValues(String savedOrigin, String message) {
        originInput.setText(savedOrigin);
        status.setText(message);
        status.setVisibility(message == null ? View.INVISIBLE : View.VISIBLE);
    }

    void setOnSubmit(Runnable action) {
        submit = action;
    }

    void setBusy(boolean value) {
        busy = value;
        originInput.setEnabled(!value);
        keyInput.setEnabled(!value);
        if (value) {
            setKeyVisible(false);
        }
        connect.setText(value ? R.string.login_connecting : R.string.login_connect);
        updateControls();
    }

    boolean isBusy() {
        return busy;
    }

    private void submit() {
        if (connect.isEnabled() && submit != null) {
            submit.run();
        }
    }

    private void setKeyVisible(boolean visible) {
        int start = keyInput.getSelectionStart();
        int end = keyInput.getSelectionEnd();
        keyVisible = visible;
        keyInput.setTransformationMethod(
            visible ? null : PasswordTransformationMethod.getInstance()
        );
        keyVisibility.setImageResource(
            visible ? R.drawable.ic_visibility_off : R.drawable.ic_visibility
        );
        keyVisibility.setContentDescription(getContext().getString(
            visible ? R.string.login_hide_key : R.string.login_show_key
        ));
        if (start >= 0 && end >= 0) {
            keyInput.setSelection(start, end);
        }
    }

    private void updateControls() {
        boolean canSubmit = !busy && originInput.length() > 0 && keyInput.length() > 0;
        connect.setEnabled(canSubmit);
        connect.setAlpha(busy || canSubmit ? 1f : .45f);
        keyVisibility.setEnabled(!busy && keyInput.length() > 0);
        keyVisibility.setAlpha(keyVisibility.isEnabled() ? 1f : .45f);
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        int horizontalPadding = getResources().getDimensionPixelSize(R.dimen.login_horizontal_padding);
        int maximumWidth = getResources().getDimensionPixelSize(R.dimen.login_max_width);
        LinearLayout.LayoutParams params = (LinearLayout.LayoutParams) panel.getLayoutParams();
        int panelWidth = Math.min(maximumWidth, Math.max(0, width - horizontalPadding * 2));
        if (params.width != panelWidth) {
            params.width = panelWidth;
            panel.setLayoutParams(params);
        }
    }
}
