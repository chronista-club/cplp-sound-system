use std::cell::RefCell;
use std::ffi::CString;

use crate::types::CplpResult;

// 最後のエラーメッセージを保持するスレッドローカルストレージ
//
// thread_local を使うことで、CString のライフタイムがスレッド存続中
// 保証される。Mutex<Option<CString>> だと guard ドロップ後にポインタが
// 無効になるダングリングポインタ問題があった。
thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// エラーメッセージを記録し、対応する CplpResult を返す
pub(crate) fn set_error(result: CplpResult, msg: impl Into<String>) -> CplpResult {
    let msg = msg.into();
    tracing::error!("FFI error: {msg}");
    if let Ok(c) = CString::new(msg) {
        LAST_ERROR.with(|cell| {
            *cell.borrow_mut() = Some(c);
        });
    }
    result
}

/// 最後のエラーメッセージを取得する（C 文字列ポインタ）
///
/// # Safety
/// 返されたポインタは次の FFI 呼び出しまで有効。呼び出し側で free してはいけない。
/// CString は thread_local に保持されるため、同一スレッド上の次の set_error
/// 呼び出しまでポインタは安全。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_last_error() -> *const std::ffi::c_char {
    LAST_ERROR.with(|cell| match &*cell.borrow() {
        Some(c) => c.as_ptr(),
        None => std::ptr::null(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    /// テスト前に LAST_ERROR をクリアするヘルパー
    fn clear_last_error() {
        LAST_ERROR.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }

    #[test]
    fn test_last_error_returns_null_when_no_error() {
        clear_last_error();

        let ptr = unsafe { cplp_last_error() };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_set_error_roundtrip() {
        clear_last_error();

        let result = set_error(CplpResult::InitError, "テストエラーメッセージ");
        assert!(matches!(result, CplpResult::InitError));

        let ptr = unsafe { cplp_last_error() };
        assert!(!ptr.is_null());

        let msg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(msg, "テストエラーメッセージ");
    }

    #[test]
    fn test_set_error_overwrites_previous() {
        clear_last_error();

        set_error(CplpResult::InitError, "最初のエラー");
        set_error(CplpResult::AudioError, "次のエラー");

        let ptr = unsafe { cplp_last_error() };
        let msg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(msg, "次のエラー");
    }

    #[test]
    fn test_set_error_returns_given_result_code() {
        clear_last_error();

        assert!(matches!(
            set_error(CplpResult::NotInitialized, "msg"),
            CplpResult::NotInitialized
        ));
        assert!(matches!(
            set_error(CplpResult::InternalError, "msg"),
            CplpResult::InternalError
        ));
        assert!(matches!(
            set_error(CplpResult::InvalidArgument, "msg"),
            CplpResult::InvalidArgument
        ));
    }

    #[test]
    fn test_set_error_with_empty_string() {
        clear_last_error();

        set_error(CplpResult::InitError, "");

        let ptr = unsafe { cplp_last_error() };
        assert!(!ptr.is_null());
        let msg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(msg, "");
    }

    #[test]
    fn test_set_error_with_nul_byte_does_not_crash() {
        clear_last_error();

        // CString::new は内部 NUL バイトでエラーを返す。
        // set_error はその場合 LAST_ERROR を更新しない。
        set_error(CplpResult::InitError, "valid message");
        set_error(CplpResult::InitError, "has\0nul");

        // NUL バイト含有文字列は CString::new が失敗するので、
        // 前のエラーメッセージが保持される
        let ptr = unsafe { cplp_last_error() };
        let msg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(msg, "valid message");
    }
}
