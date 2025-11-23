/// An idea to create a logical event bus through which all events of the system can be sent
/// This can not be implemented as a single channel because that would probably lead to too much pressure
/// and messages getting dropped when there are events for which we need to ensure delivery.
/// For example when there is an event to update 2 fans, we need to ensure that both fans receive the message.
/// To solve this there are two signals used for both fans. But they are opaque to the outside.
/// Using a single event bus should help logically bundle all channels together and prevents global static state to spread across the code base.
struct EventBus {}
