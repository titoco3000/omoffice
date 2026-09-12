
/*
To send content to the frontend, other modues use an interface that allows
them to send and receive json-encoded messages to 'channels'. For ex,
a chat module might send a chat payload to the 'chat' channel of all
participants of a certain meeting.

On the same interface, its possible to receive messages from the frontend
by subscribing a closure to a certain channel.

User beeing connected or disconected acts like a channel. 

There is only one websocket per frontend instance. Multiple channels may go through one socket.
*/ 

