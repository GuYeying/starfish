
use sdl3::{Error, IntegerOrSdlError, JoystickSubsystem as SdlJoystickSubsystem, Sdl, joystick::{Joystick, JoystickId, VirtualJoystickConnection, VirtualJoystickDescription}};



pub struct JoystickSubsystem{
    inner:SdlJoystickSubsystem,
}


impl JoystickSubsystem {

    pub fn new(sdl:&Sdl) -> Self {
        let inner = sdl.joystick().expect("JoystickSubsystem init failed.");
        Self { inner }
    }

    pub fn inner(&self)->&SdlJoystickSubsystem{
        &self.inner
    }

    pub fn joysticks(&self) -> Result<Vec<JoystickId>, Error> {
        self.inner.joysticks()
    }

    pub fn open(&self, joystick_id: JoystickId) -> Result<Joystick, IntegerOrSdlError> {
        self.inner.open(joystick_id)
    }

    pub fn set_joystick_events_enabled(&self, state: bool) {
        self.inner.set_joystick_events_enabled(state)
    }


    pub fn event_state(&self) -> bool {
        self.inner.event_state()
    }


    #[inline]
    pub fn update(&self) {
        self.inner.update()
    }


    pub fn is_virtual(&self, joystick_id: JoystickId) -> bool {
        self.inner.is_virtual(joystick_id)
    }

    /// Attach a virtual joystick
    pub fn attach_virtual_joystick(
        &self,
        desc: VirtualJoystickDescription,
    ) -> Result<VirtualJoystickConnection, IntegerOrSdlError> {
        self.inner.attach_virtual_joystick(desc,)
    }
}