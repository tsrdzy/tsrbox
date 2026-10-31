use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::{
        ffi::c_void,
        iter::once,
        mem::ManuallyDrop,
        os::windows::ffi::OsStrExt,
        path::PathBuf,
        sync::{Once, OnceLock},
    };
    use windows::{
        core::{implement, Error, Result as WinResult, HRESULT, HSTRING},
        Win32::{
            Foundation::*,
            System::{
                Com::*,
                Memory::*,
                Ole::{
                    DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, CF_HDROP, DROPEFFECT,
                    DROPEFFECT_COPY,
                },
                SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
            },
            UI::Shell::DROPFILES,
        },
    };

    static OLE_ERROR: OnceLock<String> = OnceLock::new();
    static OLE_INIT: Once = Once::new();

    fn init_ole() {
        OLE_INIT.call_once(|| unsafe {
            if let Err(error) = OleInitialize(Some(std::ptr::null_mut())) {
                let _ = OLE_ERROR.set(error.to_string());
            }
        });
    }

    #[implement(IDataObject)]
    struct FileDataObject {
        files: Vec<PathBuf>,
    }

    impl FileDataObject {
        fn is_supported_format(pformatetc: *const FORMATETC) -> bool {
            if let Some(format_etc) = unsafe { pformatetc.as_ref() } {
                !(format_etc.tymed as i32 != TYMED_HGLOBAL.0
                    || format_etc.cfFormat != CF_HDROP.0
                    || format_etc.dwAspect != DVASPECT_CONTENT.0)
            } else {
                false
            }
        }

        fn clone_drop_hglobal(&self) -> WinResult<HGLOBAL> {
            let mut buffer = Vec::new();
            for path in &self.files {
                let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
                buffer.extend(wide_path);
            }
            buffer.push(0);
            let size = std::mem::size_of::<DROPFILES>() + buffer.len() * 2;
            get_hglobal(size, buffer)
        }
    }

    #[allow(non_snake_case)]
    impl IDataObject_Impl for FileDataObject {
        fn GetData(&self, pformatetc: *const FORMATETC) -> WinResult<STGMEDIUM> {
            if Self::is_supported_format(pformatetc) {
                Ok(STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: STGMEDIUM_0 {
                        hGlobal: self.clone_drop_hglobal()?,
                    },
                    pUnkForRelease: ManuallyDrop::new(None),
                })
            } else {
                Err(Error::new(DV_E_FORMATETC, HSTRING::new()))
            }
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> WinResult<()> {
            Err(Error::new(DV_E_FORMATETC, HSTRING::new()))
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
            if Self::is_supported_format(pformatetc) {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            _pformatectin: *const FORMATETC,
            _pformatetcout: *mut FORMATETC,
        ) -> HRESULT {
            E_NOTIMPL
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: BOOL,
        ) -> WinResult<()> {
            Err(Error::new(E_NOTIMPL, HSTRING::new()))
        }

        fn EnumFormatEtc(&self, _dwdirection: u32) -> WinResult<IEnumFORMATETC> {
            Err(Error::new(E_NOTIMPL, HSTRING::new()))
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: Option<&IAdviseSink>,
        ) -> WinResult<u32> {
            Err(Error::new(OLE_E_ADVISENOTSUPPORTED, HSTRING::new()))
        }

        fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
            Err(Error::new(OLE_E_ADVISENOTSUPPORTED, HSTRING::new()))
        }

        fn EnumDAdvise(&self) -> WinResult<IEnumSTATDATA> {
            Err(Error::new(OLE_E_ADVISENOTSUPPORTED, HSTRING::new()))
        }
    }

    #[implement(IDropSource)]
    struct FileDropSource(());

    #[allow(non_snake_case)]
    impl IDropSource_Impl for FileDropSource {
        fn QueryContinueDrag(
            &self,
            fescapepressed: BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> HRESULT {
            if fescapepressed.as_bool() {
                DRAGDROP_S_CANCEL
            } else if (grfkeystate & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
                DRAGDROP_S_DROP
            } else {
                S_OK
            }
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    fn get_hglobal(size: usize, buffer: Vec<u16>) -> WinResult<HGLOBAL> {
        unsafe {
            let handle = GlobalAlloc(GMEM_FIXED, size)?;
            let ptr = GlobalLock(handle);
            let header = ptr as *mut DROPFILES;
            (*header).pFiles = std::mem::size_of::<DROPFILES>() as u32;
            (*header).fWide = BOOL(1);
            std::ptr::copy(
                buffer.as_ptr() as *const c_void,
                ptr.add(std::mem::size_of::<DROPFILES>()),
                buffer.len() * 2,
            );
            GlobalUnlock(handle)?;
            Ok(handle)
        }
    }

    pub fn start_drag(paths: Vec<PathBuf>) -> Result<(), String> {
        init_ole();
        if let Some(error) = OLE_ERROR.get() {
            return Err(format!("初始化 OLE 失败: {error}"));
        }

        let canonical_paths = paths
            .into_iter()
            .filter_map(|path| dunce::canonicalize(path).ok())
            .collect::<Vec<_>>();
        if canonical_paths.is_empty() {
            return Err("没有可拖出的文件".to_string());
        }

        let data_object: IDataObject = FileDataObject {
            files: canonical_paths,
        }
        .into();
        let drop_source: IDropSource = FileDropSource(()).into();

        unsafe {
            let mut effect = DROPEFFECT::default();
            let result = DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect);
            if result == DRAGDROP_S_DROP || result == DRAGDROP_S_CANCEL {
                Ok(())
            } else {
                Err(format!("拖出失败: {result:?}"))
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn start_drag(paths: Vec<PathBuf>) -> Result<(), String> {
    windows_impl::start_drag(paths)
}

#[cfg(not(target_os = "windows"))]
pub fn start_drag(_paths: Vec<PathBuf>) -> Result<(), String> {
    Err("当前平台暂不支持拖出文件".to_string())
}
